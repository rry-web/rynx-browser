//! Downloads: safe file names, never overwriting silently, streaming to disk, and tagging files
//! with where they came from so the OS checks them.

use super::*;

impl App {
    /// Derive a safe local filename from the last path segment of a URL, ignoring query and fragment
    pub fn download_filename(url: &str) -> String {
        let raw_name = Url::parse(url)
            .ok()
            .and_then(|u| {
                u.path_segments().and_then(|mut segments| segments.rfind(|s| !s.is_empty()).map(str::to_string))
            })
            .unwrap_or_default();
        let decoded = percent_encoding::percent_decode_str(&raw_name).decode_utf8_lossy();
        Self::sanitize_filename(&decoded)
    }

    /// Sanitize filename to prevent path traversal attacks
    fn sanitize_filename(filename: &str) -> String {
        // Get just the filename part, stripping any path components
        let path = Path::new(filename);
        let filename_only = path.file_name().and_then(|n| n.to_str()).unwrap_or("download.dat");

        // Remove any dangerous characters and ensure it's safe
        let safe_name = filename_only
            .chars()
            .map(|c| if c.is_whitespace() { '_' } else { c })
            .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
            .collect::<String>();

        // No hidden files (".bashrc"), and no trailing dots, which Windows silently drops
        let safe_name = safe_name.trim_start_matches('.').trim_end_matches('.');
        if safe_name.is_empty() {
            return "download.dat".to_string();
        }
        let safe_name = Self::limit_filename_length(safe_name);

        // On Windows, CON, NUL, COM1... (even with an extension) name devices, not files
        let stem = safe_name.split('.').next().unwrap_or("").to_ascii_uppercase();
        // COM and LPT take a digit, including the superscripts ¹²³ that Windows also accepts
        let is_device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.chars().count() == 4
                && stem.chars().nth(3).is_some_and(|c| c.is_ascii_digit() || "¹²³".contains(c)));
        if is_device { format!("_{}", safe_name) } else { safe_name }
    }

    /// Keep names well under the 255-byte limit of common filesystems, preserving the extension
    fn limit_filename_length(name: &str) -> String {
        const MAX_BYTES: usize = 200;
        if name.len() <= MAX_BYTES {
            return name.to_string();
        }
        let (stem, ext) = match name.rfind('.') {
            Some(dot) if name.len() - dot <= 20 => name.split_at(dot),
            _ => (name, ""),
        };
        let mut cut = MAX_BYTES - ext.len();
        while !stem.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}{}", &stem[..cut], ext)
    }

    /// Whether anything is at `path`, including a symbolic link (even one pointing nowhere)
    pub(super) fn path_taken(path: &Path) -> bool {
        std::fs::symlink_metadata(path).is_ok()
    }

    /// The download URL as recorded on the saved file: no credentials or #fragment
    fn origin_url(url: &str) -> String {
        match Url::parse(url) {
            Ok(mut parsed) => {
                let _ = parsed.set_username("");
                let _ = parsed.set_password(None);
                parsed.set_fragment(None);
                parsed.to_string()
            }
            Err(_) => url.to_string(),
        }
    }

    /// Tag a download as coming from the internet, as other browsers do, so the OS treats it with
    /// suspicion. macOS: the quarantine attribute, so Gatekeeper checks it before it's first
    /// opened (format: flags;hex timestamp;agent;event id).
    /// The origin is also recorded where Finder's "Where from" and the Gatekeeper dialog read it.
    #[cfg(target_os = "macos")]
    fn mark_downloaded(path: &Path, url: &str) -> std::io::Result<()> {
        let timestamp =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let value = format!("0081;{:08x};rynx;", timestamp);
        xattr::set(path, "com.apple.quarantine", value.as_bytes())?;
        let where_from = Self::binary_plist_string_array(&Self::origin_url(url));
        xattr::set(path, "com.apple.metadata:kMDItemWhereFroms", &where_from)
    }

    /// A binary property list holding a one-element array of an ASCII string (URLs serialise as
    /// ASCII), the format of kMDItemWhereFroms
    #[cfg(target_os = "macos")]
    fn binary_plist_string_array(text: &str) -> Vec<u8> {
        let mut plist = b"bplist00".to_vec();
        let array_offset = plist.len();
        plist.extend([0xA1, 0x01]); // array of 1 element: object 1
        let string_offset = plist.len();
        let bytes = text.as_bytes();
        if bytes.len() < 15 {
            plist.push(0x50 | bytes.len() as u8);
        } else {
            // Length follows as an int object: 0x13 marks an 8-byte big-endian integer
            plist.push(0x5F);
            plist.push(0x13);
            plist.extend((bytes.len() as u64).to_be_bytes());
        }
        plist.extend(bytes);
        let offset_table = plist.len();
        for offset in [array_offset, string_offset] {
            plist.extend((offset as u64).to_be_bytes());
        }
        // Trailer: 6 unused bytes, offset size, object ref size, object count, top object,
        // offset table position
        plist.extend([0u8; 6]);
        plist.extend([8, 1]);
        plist.extend(2u64.to_be_bytes());
        plist.extend(0u64.to_be_bytes());
        plist.extend((offset_table as u64).to_be_bytes());
        plist
    }

    /// Windows: the Mark of the Web, a Zone.Identifier stream saying the file came from the
    /// Internet zone, which SmartScreen and Office Protected View check
    #[cfg(windows)]
    fn mark_downloaded(path: &Path, url: &str) -> std::io::Result<()> {
        let stream = format!("{}:Zone.Identifier", path.display());
        let content = format!("[ZoneTransfer]\r\nZoneId=3\r\nHostUrl={}\r\n", Self::origin_url(url));
        std::fs::write(stream, content)
    }

    /// Linux and other Unix: the freedesktop origin attribute, shown by file managers
    #[cfg(all(unix, not(target_os = "macos")))]
    fn mark_downloaded(path: &Path, url: &str) -> std::io::Result<()> {
        xattr::set(path, "user.xdg.origin.url", Self::origin_url(url).as_bytes())
    }

    #[cfg(not(any(unix, windows)))]
    fn mark_downloaded(_path: &Path, _url: &str) -> std::io::Result<()> {
        Ok(())
    }

    /// Return `path`, or `name_1.ext`, `name_2.ext`, ... if it already exists
    fn unused_path(path: &Path) -> PathBuf {
        if !Self::path_taken(path) {
            return path.to_path_buf();
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("download");
        let ext = path.extension().and_then(|e| e.to_str());
        (1..)
            .map(|n| {
                let name = match ext {
                    Some(ext) => format!("{}_{}.{}", stem, n, ext),
                    None => format!("{}_{}", stem, n),
                };
                path.with_file_name(name)
            })
            .find(|candidate| !Self::path_taken(candidate))
            .expect("unbounded range always finds a free name")
    }

    /// Start downloading a confirmed prompt into exactly the path that was shown to the user
    pub fn trigger_download(&mut self, prompt: crate::models::DownloadPrompt) {
        let tab = self.current_tab();
        let tab_id = tab.id;
        tab.download_state = Some(crate::models::Download {
            filename: prompt.filename,
            bytes_downloaded: 0,
            total_size: None,
            status: crate::models::DownloadStatus::Active,
        });
        tab.status_message = String::from("Download starting...");

        let url = prompt.url;
        let target_path = prompt.target_path;
        let tx = self.tx.clone();
        let use_i2p = self.i2p_mode;
        let nm = Arc::clone(&self.network_manager);

        tokio::spawn(async move {
            let client = nm.get_download_client(use_i2p);

            // 1. Send request
            let res = match crate::network::get_following_redirects(client, &url, use_i2p).await {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(NetworkResponse::DownloadError(tab_id, format!("Connection failed: {}", e))).await;
                    return;
                }
            };

            // Don't save a server's error page as if it were the file
            if !res.status().is_success() {
                let _ =
                    tx.send(NetworkResponse::DownloadError(tab_id, format!("Server returned {}", res.status()))).await;
                return;
            }

            let total = res.content_length();
            let mut stream = res.bytes_stream();
            let mut downloaded: u64 = 0;
            let mut file_handle: Option<(tokio::fs::File, String)> = None;
            // Throttle progress updates so the UI channel never limits download speed
            let progress_interval = Duration::from_millis(DOWNLOAD_PROGRESS_INTERVAL_MS);
            let mut last_progress: Option<Instant> = None;

            while let Some(item) = stream.next().await {
                let chunk = match item {
                    Ok(chunk) => chunk,
                    Err(e) => {
                        let _ = tx
                            .send(NetworkResponse::DownloadError(tab_id, format!("Download interrupted: {}", e)))
                            .await;
                        return;
                    }
                };
                if file_handle.is_none() {
                    // 2. Create Downloads Directory
                    if let Some(dir) = target_path.parent()
                        && let Err(e) = tokio::fs::create_dir_all(dir).await
                    {
                        let _ = tx.send(NetworkResponse::DownloadError(tab_id, format!("Folder error: {}", e))).await;
                        return;
                    }

                    // 3. Sniff Extension; only the confirmed path may be overwritten
                    let mut file_path = target_path.clone();
                    if file_path.extension().is_none()
                        && let Some(ext) = crate::network::sniff_extension(&chunk)
                    {
                        file_path = Self::unused_path(&file_path.with_extension(ext));
                    }

                    // 4. Safe File Creation (No unwraps). Never write through a symbolic link: one
                    // planted in Downloads could point the write at e.g. ~/.bashrc
                    let is_symlink =
                        tokio::fs::symlink_metadata(&file_path).await.is_ok_and(|meta| meta.file_type().is_symlink());
                    if is_symlink {
                        let _ = tx
                            .send(NetworkResponse::DownloadError(
                                tab_id,
                                "Refusing to save over a symbolic link".to_string(),
                            ))
                            .await;
                        return;
                    }
                    let file = match tokio::fs::File::create(&file_path).await {
                        Ok(f) => f,
                        Err(e) => {
                            let _ = tx.send(NetworkResponse::DownloadError(tab_id, format!("File error: {}", e))).await;
                            return;
                        }
                    };
                    // Tag before writing, so even a partial download is checked on first open.
                    // Best effort: some filesystems (e.g. FAT USB drives) can't store the tag.
                    let _ = Self::mark_downloaded(&file_path, &url);
                    file_handle = Some((file, file_path.display().to_string()));
                }

                if let Some((ref mut file, _)) = file_handle {
                    if let Err(e) = file.write_all(&chunk).await {
                        let _ = tx.send(NetworkResponse::DownloadError(tab_id, format!("Write failed: {}", e))).await;
                        return;
                    }
                    downloaded += chunk.len() as u64;
                    if last_progress.is_none_or(|t| t.elapsed() >= progress_interval) {
                        last_progress = Some(Instant::now());
                        let _ = tx.send(NetworkResponse::DownloadProgress(tab_id, downloaded, total)).await;
                    }
                }
            }

            if let Some((mut file, path)) = file_handle {
                // tokio completes writes in the background; flush so the file is whole before reporting done
                if let Err(e) = file.flush().await {
                    let _ = tx.send(NetworkResponse::DownloadError(tab_id, format!("Write failed: {}", e))).await;
                    return;
                }
                let _ = tx.send(NetworkResponse::DownloadFinished(tab_id, path)).await;
            } else {
                let _ = tx.send(NetworkResponse::DownloadError(tab_id, "Download stream was empty".to_string())).await;
            }
        });
    }
}

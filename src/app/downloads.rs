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
            bytes_downloaded: prompt.resume_from.unwrap_or(0),
            total_size: None,
            status: crate::models::DownloadStatus::Active,
        });
        tab.status_message = String::from(if prompt.resume_from.is_some() {
            "Resuming the download..."
        } else {
            "Download starting..."
        });

        let tx = self.tx.clone();
        let use_i2p = self.i2p_mode;
        let nm = Arc::clone(&self.network_manager);
        tokio::spawn(async move {
            // A file on a NomadNet node comes through the Reticulum bridge
            let (url, use_i2p) = match crate::reticulum::bridge_url(&prompt.url) {
                Some(bridged) => (bridged, false),
                None => (prompt.url.clone(), use_i2p),
            };
            let client = nm.get_download_client(use_i2p);
            let message = match download(client, &url, &prompt.target_path, use_i2p, &tx, tab_id).await {
                Ok(path) => NetworkResponse::DownloadFinished(tab_id, path),
                Err(e) => NetworkResponse::DownloadError(tab_id, e),
            };
            let _ = tx.send(message).await;
        });
    }
}

/// Times an interrupted download is resumed by itself before giving up (it can still be resumed
/// later, by downloading the same link again)
const RESUME_ATTEMPTS: u32 = 5;

/// Where a download in progress is written, and the note of what's needed to resume it
fn partial_paths(target: &Path) -> (PathBuf, PathBuf) {
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    (target.with_file_name(format!("{}.part", name)), target.with_file_name(format!("{}.part.json", name)))
}

/// The bytes already downloaded of `url` into `target` by an earlier, interrupted download
pub(crate) fn partial_download(target: &Path, url: &str) -> Option<u64> {
    let (part, note) = partial_paths(target);
    let note: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(note).ok()?).ok()?;
    if note.get("url")?.as_str()? != url {
        return None;
    }
    let length = std::fs::symlink_metadata(&part).ok().filter(|m| m.is_file())?.len();
    (length > 0).then_some(length)
}

/// Why a download attempt stopped: worth resuming, or not
enum Stopped {
    Resumable(String),
    Fatal(String),
}

/// "bytes 400-999/1000": the first byte and the total
fn content_range(response: &reqwest::Response) -> Option<(u64, Option<u64>)> {
    let value = response.headers().get(reqwest::header::CONTENT_RANGE)?.to_str().ok()?;
    let (range, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let start = range.split_once('-').map(|(s, _)| s).unwrap_or(range).parse().ok();
    Some((start.unwrap_or(0), total.parse().ok()))
}

/// Download `url` to `target`, through `target.part`, resuming where an earlier attempt stopped
/// and by itself when the connection breaks. The validator (ETag or Last-Modified) goes with each
/// resume as If-Range, so a file changed on the server is downloaded afresh, never spliced.
async fn download(
    client: &reqwest::Client,
    url: &str,
    target: &Path,
    i2p_mode: bool,
    tx: &mpsc::Sender<NetworkResponse>,
    tab_id: usize,
) -> Result<String, String> {
    let (part, note) = partial_paths(target);
    if let Some(dir) = target.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(|e| format!("Folder error: {}", e))?;
    }
    // Never write through a symbolic link: one planted in Downloads could point the write at
    // e.g. ~/.bashrc
    for path in [&part, &note, &target.to_path_buf()] {
        if tokio::fs::symlink_metadata(path).await.is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("Refusing to save over a symbolic link".to_string());
        }
    }
    let mut have = partial_download(target, url).unwrap_or(0);
    let mut validator: Option<String> = None;
    if have > 0 {
        let saved: Option<serde_json::Value> =
            std::fs::read_to_string(&note).ok().and_then(|t| serde_json::from_str(&t).ok());
        validator = saved.and_then(|v| v.get("validator").and_then(|v| v.as_str()).map(str::to_string));
    } else {
        // Leftovers of a download of something else
        let _ = tokio::fs::remove_file(&part).await;
        let _ = tokio::fs::remove_file(&note).await;
    }

    let mut attempt = 0;
    loop {
        let result = download_attempt(client, url, i2p_mode, &part, &note, &mut have, &mut validator, tx, tab_id).await;
        match result {
            Ok(()) => break,
            Err(Stopped::Resumable(e)) if attempt < RESUME_ATTEMPTS => {
                attempt += 1;
                let message =
                    format!("Download interrupted ({}); resuming, attempt {} of {}...", e, attempt, RESUME_ATTEMPTS);
                let _ = tx.send(NetworkResponse::Info(tab_id, message)).await;
                tokio::time::sleep(Duration::from_secs(2 * attempt as u64)).await;
            }
            Err(Stopped::Resumable(e)) | Err(Stopped::Fatal(e)) => {
                return Err(if have > 0 {
                    format!("{}. What was downloaded is kept: download the link again to resume", e)
                } else {
                    e
                });
            }
        }
    }

    // The file's own extension, when the address didn't give one
    let mut destination = target.to_path_buf();
    if destination.extension().is_none() {
        let mut head = vec![0u8; 64];
        let read = match tokio::fs::File::open(&part).await {
            Ok(mut file) => tokio::io::AsyncReadExt::read(&mut file, &mut head).await.unwrap_or(0),
            Err(_) => 0,
        };
        if let Some(ext) = crate::network::sniff_extension(&head[..read]) {
            destination = App::unused_path(&destination.with_extension(ext));
        }
    }
    if tokio::fs::symlink_metadata(&destination).await.is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("Refusing to save over a symbolic link".to_string());
    }
    // Only the confirmed path, or a new one, is written: the rename replaces only what was agreed
    tokio::fs::rename(&part, &destination).await.map_err(|e| format!("Couldn't finish the file: {}", e))?;
    let _ = tokio::fs::remove_file(&note).await;
    let _ = App::mark_downloaded(&destination, url);
    Ok(destination.display().to_string())
}

/// One request: the whole file, or the rest of it from `have`
#[allow(clippy::too_many_arguments)]
async fn download_attempt(
    client: &reqwest::Client,
    url: &str,
    i2p_mode: bool,
    part: &Path,
    note: &Path,
    have: &mut u64,
    validator: &mut Option<String>,
    tx: &mpsc::Sender<NetworkResponse>,
    tab_id: usize,
) -> Result<(), Stopped> {
    use reqwest::header::{HeaderValue, IF_RANGE, RANGE};
    let mut headers = reqwest::header::HeaderMap::new();
    if *have > 0 {
        if let Ok(range) = HeaderValue::from_str(&format!("bytes={}-", have)) {
            headers.insert(RANGE, range);
        }
        if let Some(value) = validator.as_deref().and_then(|v| HeaderValue::from_str(v).ok()) {
            headers.insert(IF_RANGE, value);
        }
    }
    let resuming = *have > 0;
    let response = crate::network::send_with_headers(client, url, i2p_mode, None, headers).await.map_err(|e| {
        let message = format!("Connection failed: {}", e);
        if resuming { Stopped::Resumable(message) } else { Stopped::Fatal(message) }
    })?;

    let status = response.status();
    let mut total = None;
    let append = match status {
        reqwest::StatusCode::PARTIAL_CONTENT if resuming => match content_range(&response) {
            Some((start, whole)) if start == *have => {
                total = whole;
                true
            }
            // Not the part asked for: start again rather than splice the wrong bytes
            _ => return Err(Stopped::Resumable("the server sent the wrong part".to_string())),
        },
        // Nothing left to send: it was all there already
        reqwest::StatusCode::RANGE_NOT_SATISFIABLE if resuming => {
            if content_range(&response).and_then(|(_, whole)| whole) == Some(*have) {
                return Ok(());
            }
            *have = 0;
            return Err(Stopped::Resumable("the server couldn't continue".to_string()));
        }
        // The whole file: new, or changed since the earlier attempt, or the server can't resume
        s if s.is_success() => false,
        // Don't save a server's error page as if it were the file
        s => return Err(Stopped::Fatal(format!("Server returned {}", s))),
    };
    if !append {
        *have = 0;
        total = response.content_length();
    }

    // Remember how to resume this download, in case it's interrupted
    let headers = response.headers();
    let text = |name| headers.get(name).and_then(|v: &HeaderValue| v.to_str().ok()).map(str::to_string);
    *validator =
        text(reqwest::header::ETAG).filter(|e| !e.starts_with("W/")).or_else(|| text(reqwest::header::LAST_MODIFIED));
    let saved = serde_json::json!({ "url": url, "validator": validator });
    tokio::fs::write(note, saved.to_string()).await.map_err(|e| Stopped::Fatal(format!("File error: {}", e)))?;

    let mut file = if append {
        tokio::fs::OpenOptions::new().append(true).open(part).await
    } else {
        tokio::fs::File::create(part).await
    }
    .map_err(|e| Stopped::Fatal(format!("File error: {}", e)))?;
    if !append {
        // Tag before writing, so even a partial download is checked on first open. Best effort:
        // some filesystems (e.g. FAT USB drives) can't store the tag.
        let _ = App::mark_downloaded(part, url);
    }

    let mut stream = response.bytes_stream();
    // Throttle progress updates so the UI channel never limits download speed
    let progress_interval = Duration::from_millis(DOWNLOAD_PROGRESS_INTERVAL_MS);
    let mut last_progress: Option<Instant> = None;
    let mut received_any = false;
    while let Some(item) = stream.next().await {
        let chunk = match item {
            Ok(chunk) => chunk,
            Err(e) => {
                let _ = file.flush().await;
                return Err(Stopped::Resumable(format!("interrupted: {}", e)));
            }
        };
        received_any = true;
        file.write_all(&chunk).await.map_err(|e| Stopped::Fatal(format!("Write failed: {}", e)))?;
        *have += chunk.len() as u64;
        if last_progress.is_none_or(|t| t.elapsed() >= progress_interval) {
            last_progress = Some(Instant::now());
            let _ = tx.send(NetworkResponse::DownloadProgress(tab_id, *have, total)).await;
        }
    }
    // tokio completes writes in the background; flush so the file is whole before it's renamed
    file.flush().await.map_err(|e| Stopped::Fatal(format!("Write failed: {}", e)))?;
    if !received_any && *have == 0 {
        return Err(Stopped::Fatal("Download stream was empty".to_string()));
    }
    // A connection that closed early, without an error, still left the file short
    if total.is_some_and(|t| *have < t) {
        return Err(Stopped::Resumable(format!("the connection closed at {} of {} bytes", have, total.unwrap_or(0))));
    }
    Ok(())
}

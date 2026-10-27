//! Updates: finding a newer release on GitHub, downloading this platform's build of it once the
//! user agrees, and installing it the next time rynx starts.
//!
//! An update is installed only if the release's checksum list (SHA256SUMS) carries a valid
//! Ed25519 signature from the release key, whose public half is built into rynx, and the program
//! downloaded has the checksum the list gives. The checksums alone would only catch a damaged
//! download: whoever could change a release's files could change its checksums too, but not
//! sign them without the key, which only the release workflow holds.
//!
//! While rynx runs, its sandbox keeps it from writing where its own program file is, so the
//! update is downloaded to the cache folder and put in place at the next start, before the
//! sandbox applies. Copies installed with Cargo or Homebrew are left to those tools.

use sha_digest::sha256_hex;
use std::path::{Path, PathBuf};

/// The release key's public half: Ed25519, 32 bytes (also in .github/release-signing-key.pub.pem)
pub const RELEASE_KEY: [u8; 32] = [
    0x05, 0x4a, 0x62, 0x30, 0x9e, 0x79, 0xd8, 0xe2, 0xe7, 0x12, 0x21, 0xfe, 0xde, 0x82, 0x5f, 0xf7, 0x8d, 0x02, 0xa7,
    0xdf, 0x0e, 0x84, 0x28, 0x77, 0x4e, 0x43, 0x2a, 0x6a, 0x67, 0x5f, 0xcb, 0xd7,
];

/// Where releases are listed
pub const RELEASES_API: &str = "https://api.github.com/repos/rry-web/rynx-browser/releases?per_page=20";

/// The platform this program was built for, as release builds are named
pub const TARGET: &str = env!("RYNX_TARGET");

/// The update page
pub const UPDATE_PAGE: &str = "about:update";

/// Checked once a day at most
pub const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;
pub const LAST_CHECK_FILE: &str = "update_checked.txt";

const MAX_PROGRAM_BYTES: u64 = 64 * 1024 * 1024;
const MAX_LIST_BYTES: u64 = 2 * 1024 * 1024;

mod sha_digest {
    pub fn sha256_hex(data: &[u8]) -> String {
        ring::digest::digest(&ring::digest::SHA256, data).as_ref().iter().map(|b| format!("{:02x}", b)).collect()
    }
}

/// A release newer than this one, with this platform's build
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: semver::Version,
    /// The release notes, as written (Markdown)
    pub notes: String,
    pub page: String,
    pub program_url: String,
    pub sums_url: String,
    pub signature_url: String,
}

/// How this copy of rynx was installed, which decides how it's updated
#[derive(Clone, Debug, PartialEq)]
pub enum Installed {
    /// A release build in a folder rynx can write to: it updates itself
    Updatable(PathBuf),
    /// With cargo install: `cargo install` updates it
    Cargo,
    /// With Homebrew: `brew upgrade` updates it
    Homebrew,
    /// Somewhere rynx can't replace itself (e.g. a system folder), or a development build
    Elsewhere(String),
}

pub fn current_version() -> semver::Version {
    semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("the crate version is semver")
}

/// The file name of a release's build for this platform
pub fn program_name(version: &semver::Version) -> String {
    let exe = if TARGET.contains("windows") { ".exe" } else { "" };
    format!("rynx-{}-{}{}", version, TARGET, exe)
}

/// The newest release in GitHub's release list that's newer than `current`, has a build for this
/// platform and a signed checksum list; pre-releases only if `betas`
pub fn newest(json: &str, current: &semver::Version, betas: bool) -> Option<Release> {
    let releases: serde_json::Value = serde_json::from_str(json).ok()?;
    let mut best: Option<Release> = None;
    for release in releases.as_array()? {
        let flag = |name| release.get(name).and_then(|v| v.as_bool()).unwrap_or(false);
        if flag("draft") || (flag("prerelease") && !betas) {
            continue;
        }
        let Some(version) = release
            .get("tag_name")
            .and_then(|t| t.as_str())
            .and_then(|t| semver::Version::parse(t.trim_start_matches('v')).ok())
        else {
            continue;
        };
        if version <= *current || (!betas && !version.pre.is_empty()) {
            continue;
        }
        let assets = release.get("assets").and_then(|a| a.as_array()).cloned().unwrap_or_default();
        let url_of = |name: &str| {
            assets
                .iter()
                .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(name))
                .and_then(|a| a.get("browser_download_url").and_then(|u| u.as_str()))
                .map(str::to_string)
        };
        let (Some(program_url), Some(sums_url), Some(signature_url)) =
            (url_of(&program_name(&version)), url_of("SHA256SUMS"), url_of("SHA256SUMS.sig"))
        else {
            continue;
        };
        if best.as_ref().is_some_and(|b| b.version >= version) {
            continue;
        }
        let text = |name| release.get(name).and_then(|v| v.as_str()).unwrap_or("").to_string();
        best = Some(Release {
            version,
            notes: text("body"),
            page: text("html_url"),
            program_url,
            sums_url,
            signature_url,
        });
    }
    best
}

/// Whether `signature` is the release key's (`key`) Ed25519 signature of `message`
pub fn verify(message: &[u8], signature: &[u8], key: &[u8; 32]) -> bool {
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key).verify(message, signature).is_ok()
}

/// The checksum a SHA256SUMS list gives `name` ("<hex>  <name>" lines)
pub fn checksum_for(list: &str, name: &str) -> Option<String> {
    list.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name && hash.len() == 64).then(|| hash.to_ascii_lowercase())
    })
}

/// How this copy was installed. Called before the sandbox applies, since it checks whether the
/// program's folder can be written to.
pub fn installed() -> Installed {
    let Ok(exe) = std::env::current_exe().and_then(|p| p.canonicalize()) else {
        return Installed::Elsewhere("rynx couldn't find its own program file".to_string());
    };
    let path = exe.to_string_lossy();
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|d| d.home_dir().join(".cargo")));
    if cargo_home.is_some_and(|home| exe.starts_with(home.join("bin"))) {
        return Installed::Cargo;
    }
    if path.contains("/Cellar/") || path.contains("/homebrew/") || path.contains("/linuxbrew/") {
        return Installed::Homebrew;
    }
    if path.contains("/target/debug/") || path.contains("/target/release/") {
        return Installed::Elsewhere("this is a development build".to_string());
    }
    let Some(folder) = exe.parent() else {
        return Installed::Elsewhere("rynx couldn't find its own folder".to_string());
    };
    let probe = folder.join(format!(".rynx-update-check-{}", std::process::id()));
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            Installed::Updatable(exe)
        }
        Err(_) => Installed::Elsewhere(format!("rynx can't write to {}", folder.display())),
    }
}

/// An update downloaded and verified, waiting for the next start
#[derive(Clone, Debug, PartialEq)]
pub struct Pending {
    pub version: String,
    pub file: PathBuf,
    pub sha256: String,
}

const PENDING_FILE: &str = "pending.json";

/// The update waiting in `dir`, if any
pub fn pending(dir: &Path) -> Option<Pending> {
    let note: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join(PENDING_FILE)).ok()?).ok()?;
    let text = |name: &str| note.get(name).and_then(|v| v.as_str()).map(str::to_string);
    Some(Pending { version: text("version")?, file: dir.join(text("file")?), sha256: text("sha256")? })
}

/// Forget a waiting update
pub fn cancel(dir: &Path) {
    if let Some(pending) = pending(dir) {
        let _ = std::fs::remove_file(pending.file);
    }
    let _ = std::fs::remove_file(dir.join(PENDING_FILE));
}

async fn fetch(client: &reqwest::Client, url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let response = crate::network::get_following_redirects(client, url, false).await?;
    if !response.status().is_success() {
        return Err(format!("GitHub answered {}", response.status()));
    }
    crate::network::read_bytes_limited(response, limit).await
}

/// Ask GitHub for a release newer than this one
pub async fn check(client: &reqwest::Client, api: &str, betas: bool) -> Result<Option<Release>, String> {
    let list = fetch(client, api, MAX_LIST_BYTES).await?;
    Ok(newest(&String::from_utf8_lossy(&list), &current_version(), betas))
}

/// Download a release's build for this platform into `dir`, checking the signature of its
/// checksum list and its checksum, and leave it to be installed at the next start
pub async fn download(
    client: &reqwest::Client,
    release: &Release,
    dir: &Path,
    key: &[u8; 32],
) -> Result<Pending, String> {
    let sums = fetch(client, &release.sums_url, MAX_LIST_BYTES).await?;
    let signature = fetch(client, &release.signature_url, 1024).await?;
    if !verify(&sums, &signature, key) {
        return Err("the release's signature isn't valid, so it wasn't installed".to_string());
    }
    let name = program_name(&release.version);
    let expected = checksum_for(&String::from_utf8_lossy(&sums), &name)
        .ok_or("the release's signed checksums don't list this platform's build")?;
    let program = fetch(client, &release.program_url, MAX_PROGRAM_BYTES).await?;
    if sha256_hex(&program) != expected {
        return Err("the download doesn't match the release's signed checksum, so it wasn't installed".to_string());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't save the update: {}", e))?;
    let file = dir.join(&name);
    std::fs::write(&file, &program).map_err(|e| format!("Couldn't save the update: {}", e))?;
    let note = serde_json::json!({ "version": release.version.to_string(), "file": name, "sha256": expected });
    std::fs::write(dir.join(PENDING_FILE), note.to_string()).map_err(|e| format!("Couldn't save the update: {}", e))?;
    Ok(Pending { version: release.version.to_string(), file, sha256: expected })
}

/// Install the update waiting in `dir` over `program` (this program's file; None for the
/// running program). Returns the version installed, if there was one. The file's checksum is
/// checked again first, in case it changed since it was downloaded.
pub fn install_pending(dir: &Path, program: Option<&Path>) -> Result<Option<String>, String> {
    let Some(pending) = pending(dir) else { return Ok(None) };
    let data = std::fs::read(&pending.file).map_err(|e| format!("The downloaded update is missing: {}", e))?;
    if sha256_hex(&data) != pending.sha256 {
        cancel(dir);
        return Err("the downloaded update changed since it was checked, so it wasn't installed".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&pending.file, std::fs::Permissions::from_mode(0o755));
    }
    match program {
        Some(program) => {
            // Next to the program, then renamed over it: an interrupted install leaves it whole
            let staged = program.with_extension("update");
            std::fs::copy(&pending.file, &staged)
                .and_then(|_| std::fs::rename(&staged, program))
                .map_err(|e| format!("Couldn't install the update: {}", e))?;
        }
        None => self_replace::self_replace(&pending.file).map_err(|e| format!("Couldn't install the update: {}", e))?,
    }
    cancel(dir);
    Ok(Some(pending.version))
}

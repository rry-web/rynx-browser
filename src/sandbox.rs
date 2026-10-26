//! Restricts what the browser can touch on disk once it's running.
//!
//! On Linux this uses Landlock: the browser may read system files (libraries, DNS settings,
//! certificates) and use the terminal, but may only write inside the Downloads folder and its own
//! settings folder (settings, bookmarks and history), and can't
//! read the rest of the home directory (SSH keys, other programs' data). A bug in page parsing or
//! the network stack can't be used to steal files or plant e.g. ~/.bashrc.
//!
//! Landlock restricts the calling thread and threads it starts later, so this must run before
//! any other thread exists: before the async runtime, the clipboard and background rendering.

use std::path::Path;

/// Outcome of trying to sandbox the browser, shown in the status line at startup
#[derive(Debug, Clone, PartialEq)]
pub enum SandboxStatus {
    /// Every requested restriction is in force
    Enforced,
    /// The kernel supports only some restrictions (an older Landlock version)
    Partial,
    /// The kernel has no Landlock support
    Unavailable,
    /// Setting up the sandbox failed
    Failed(String),
    /// Turned off with --no-sandbox
    Disabled,
    /// No sandbox is implemented for this platform
    Unsupported,
}

impl SandboxStatus {
    pub fn describe(&self) -> String {
        match self {
            Self::Enforced => "filesystem sandbox active".to_string(),
            Self::Partial => "filesystem sandbox partly active (older kernel)".to_string(),
            Self::Unavailable => "no filesystem sandbox: kernel lacks Landlock".to_string(),
            Self::Failed(reason) => format!("filesystem sandbox failed: {}", reason),
            Self::Disabled => "filesystem sandbox disabled (--no-sandbox)".to_string(),
            Self::Unsupported => String::new(),
        }
    }
}

/// System locations the browser reads: shared libraries, DNS and TLS configuration, its own
/// process information. Paths that don't exist on this system are skipped.
#[cfg(target_os = "linux")]
const READ_ONLY_PATHS: &[&str] = &[
    "/usr",
    "/lib",
    "/lib32",
    "/lib64",
    "/libx32",
    "/bin",
    "/sbin",
    "/etc",
    "/var/lib/ca-certificates",
    "/run/systemd/resolve",
    "/nix/store",
    "/gnu/store",
    "/proc/self",
    "/sys/fs/cgroup",
    "/sys/devices/system/cpu",
    "/dev/urandom",
];

/// Devices the terminal UI opens for reading, writing and terminal control
#[cfg(target_os = "linux")]
const DEVICE_PATHS: &[&str] = &["/dev/tty", "/dev/null"];

/// Restrict filesystem access to what the browser needs, allowing writes only in `writable`
/// folders (Downloads and the settings folder)
#[cfg(target_os = "linux")]
pub fn restrict_filesystem(writable: &[&Path]) -> SandboxStatus {
    use landlock::{
        ABI, Access, AccessFs, Ruleset, RulesetAttr, RulesetCreatedAttr, RulesetStatus,
        path_beneath_rules,
    };

    let abi = ABI::V5;
    let read = AccessFs::from_read(abi);
    let all = AccessFs::from_all(abi);

    // Certificate locations and the X11 auth file can be configured outside the system paths
    let mut extra_read: Vec<std::path::PathBuf> = Vec::new();
    for var in ["SSL_CERT_FILE", "SSL_CERT_DIR"] {
        if let Some(value) = std::env::var_os(var) {
            extra_read.extend(std::env::split_paths(&value));
        }
    }
    match std::env::var_os("XAUTHORITY") {
        Some(path) => extra_read.push(path.into()),
        None => {
            if let Some(home) = std::env::var_os("HOME") {
                extra_read.push(Path::new(&home).join(".Xauthority"));
            }
        }
    }

    let result = Ruleset::default()
        .handle_access(all)
        .and_then(|ruleset| ruleset.create())
        .and_then(|ruleset| ruleset.add_rules(path_beneath_rules(READ_ONLY_PATHS, read)))
        .and_then(|ruleset| ruleset.add_rules(path_beneath_rules(&extra_read, read)))
        .and_then(|ruleset| ruleset.add_rules(path_beneath_rules(DEVICE_PATHS, all)))
        .and_then(|ruleset| ruleset.add_rules(path_beneath_rules(writable, all)))
        .and_then(|ruleset| ruleset.restrict_self());

    match result {
        Ok(status) => match status.ruleset {
            RulesetStatus::FullyEnforced => SandboxStatus::Enforced,
            RulesetStatus::PartiallyEnforced => SandboxStatus::Partial,
            RulesetStatus::NotEnforced => SandboxStatus::Unavailable,
        },
        Err(e) => SandboxStatus::Failed(e.to_string()),
    }
}

#[cfg(not(target_os = "linux"))]
pub fn restrict_filesystem(_writable: &[&Path]) -> SandboxStatus {
    SandboxStatus::Unsupported
}

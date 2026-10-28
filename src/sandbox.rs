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
//!
//! On macOS it uses Seatbelt, the sandbox Safari, Chrome and Firefox confine their page processes
//! with, with the same rules for files: no reading or writing in the home folder except the
//! Downloads and settings folders, and no writing anywhere else except the terminal's devices.
//! Unlike Landlock it applies to the whole process at once; programs it starts inherit it. The
//! policy starts from "allow" and denies file access, rather than listing every system service the
//! browser uses (certificate checks, DNS, the clipboard), which differ between macOS versions.
//!
//! On both, once sandboxed the browser can't start programs (it never does: the I2P router and
//! the restart after an update happen before the sandbox) or listen for connections (it only
//! makes them), so a bug that let a page take over the browser couldn't run a shell or open a
//! port. Crash dumps, which would hold pages and cookies, are turned off for every run.

use std::path::Path;

/// Outcome of trying to sandbox the browser, shown in the status line at startup
#[derive(Debug, Clone, PartialEq)]
pub enum SandboxStatus {
    /// Every requested restriction is in force
    Enforced,
    /// The kernel supports only some restrictions (an older Landlock version)
    Partial,
    /// The operating system doesn't support this sandbox (no Landlock in the kernel)
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
        ABI, Access, AccessFs, AccessNet, Ruleset, RulesetAttr, RulesetCreatedAttr, RulesetStatus, path_beneath_rules,
    };

    let abi = ABI::V5;
    // Running programs is allowed nowhere: Execute is handled, and granted by no rule
    let read = AccessFs::from_read(abi) & !AccessFs::Execute;
    let all = AccessFs::from_all(abi);
    let writable_access = all & !AccessFs::Execute;

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
        // Listening on a port is allowed nowhere (no rule grants it); connecting isn't restricted
        .and_then(|ruleset| ruleset.handle_access(AccessNet::BindTcp))
        .and_then(|ruleset| ruleset.create())
        .and_then(|ruleset| ruleset.add_rules(path_beneath_rules(READ_ONLY_PATHS, read)))
        .and_then(|ruleset| ruleset.add_rules(path_beneath_rules(&extra_read, read)))
        .and_then(|ruleset| ruleset.add_rules(path_beneath_rules(DEVICE_PATHS, writable_access)))
        .and_then(|ruleset| ruleset.add_rules(path_beneath_rules(writable, writable_access)))
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

/// The Seatbelt policy: reading and writing in `home` only inside `writable`, and writing
/// elsewhere only to devices (the terminal, /dev/null). The browser's own executable and the
/// listing of its folder stay readable: macOS's certificate checks look at the running program
/// (and fail outright without it), and a `cargo install`ed rynx lives in ~/.cargo/bin.
#[cfg(target_os = "macos")]
fn seatbelt_policy(home: &Path, writable: &[&Path], executable: Option<&Path>) -> String {
    // Paths go in as string literals; Seatbelt compares real paths, so symlinks are resolved
    let quote = |path: &Path| {
        let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        format!("\"{}\"", real.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\""))
    };
    let allowed: String = writable.iter().map(|dir| format!(" (subpath {})", quote(dir))).collect();
    let home = quote(home);
    let mut policy = format!(
        "(version 1)\n\
         (allow default)\n\
         (deny file-write*)\n\
         (allow file-write* (subpath \"/dev\"){allowed})\n\
         (deny file-read* (subpath {home}))\n\
         (allow file-read-metadata (literal {home}))\n\
         (deny process-exec*)\n\
         (deny process-fork)\n\
         (deny network-bind)\n"
    );
    // Only with folders to name: "(allow file-read*)" alone would allow reading everything
    if !allowed.is_empty() {
        policy.push_str(&format!("(allow file-read*{allowed})\n"));
    }
    if let Some(executable) = executable {
        let folder = executable.parent().map(|f| format!(" (literal {})", quote(f))).unwrap_or_default();
        policy.push_str(&format!("(allow file-read* (literal {}){folder})\n", quote(executable)));
    }
    policy
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::seatbelt_policy;
    use std::path::Path;

    #[test]
    fn the_policy_never_allows_all_reads() {
        let policy = seatbelt_policy(Path::new("/Users/nobody"), &[], None);
        assert!(!policy.contains("(allow file-read*)"), "{policy}");
        let policy = seatbelt_policy(Path::new("/Users/nobody"), &[Path::new("/Users/nobody/Down\"loads")], None);
        assert!(policy.contains("(allow file-read* (subpath \"/Users/nobody/Down\\\"loads\"))"), "{policy}");
    }
}

/// Restrict filesystem access to what the browser needs, allowing writes only in `writable`
/// folders (Downloads and the settings folder), and reads in the home folder only there
#[cfg(target_os = "macos")]
pub fn restrict_filesystem(writable: &[&Path]) -> SandboxStatus {
    let home = match std::env::var_os("HOME").filter(|h| !h.is_empty()) {
        Some(home) => std::path::PathBuf::from(home),
        None => match directories::BaseDirs::new() {
            Some(dirs) => dirs.home_dir().to_path_buf(),
            None => return SandboxStatus::Failed("no home folder".to_string()),
        },
    };
    let executable = std::env::current_exe().ok();
    match seatbelt::apply(&seatbelt_policy(&home, writable, executable.as_deref())) {
        Ok(()) => SandboxStatus::Enforced,
        Err(e) => SandboxStatus::Failed(e),
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod seatbelt {
    use std::ffi::{CStr, CString, c_char};

    // libsandbox, part of libSystem. Deprecated in its header since macOS 10.8, but it's the same
    // interface the major browsers' sandboxes are built on, and still works.
    unsafe extern "C" {
        fn sandbox_init(profile: *const c_char, flags: u64, errorbuf: *mut *mut c_char) -> i32;
        fn sandbox_free_error(errorbuf: *mut c_char);
    }

    /// Apply a policy (Seatbelt's profile language) to this process, irreversibly
    pub fn apply(policy: &str) -> Result<(), String> {
        let policy = CString::new(policy).map_err(|_| "policy contains a NUL byte".to_string())?;
        let mut error: *mut c_char = std::ptr::null_mut();
        // SAFETY: the policy is a valid C string for the call's duration; flags 0 means it's a
        // policy text rather than a named profile; on failure the error string is ours to free.
        let result = unsafe { sandbox_init(policy.as_ptr(), 0, &mut error) };
        if result == 0 {
            return Ok(());
        }
        let message = if error.is_null() {
            "sandbox_init failed".to_string()
        } else {
            // SAFETY: a non-null error is a C string from libsandbox, freed once here
            let text = unsafe { CStr::from_ptr(error) }.to_string_lossy().into_owned();
            unsafe { sandbox_free_error(error) };
            text
        };
        Err(message)
    }
}

/// Turn crash dumps off: one would hold the pages, cookies and passwords in memory
#[cfg(unix)]
#[allow(unsafe_code)]
pub fn disable_core_dumps() {
    let none = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: setrlimit reads the struct passed, which lives for the call
    unsafe {
        libc::setrlimit(libc::RLIMIT_CORE, &none);
    }
}

#[cfg(not(unix))]
pub fn disable_core_dumps() {}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn restrict_filesystem(_writable: &[&Path]) -> SandboxStatus {
    SandboxStatus::Unsupported
}

//! The Linux filesystem sandbox. Restrictions can't be lifted once applied, so the test re-runs
//! itself in a child process that sandboxes itself and reports what it could and couldn't do.
#![cfg(target_os = "linux")]

use rynx::sandbox::{SandboxStatus, restrict_filesystem};
use std::path::{Path, PathBuf};
use std::process::Command;

const CHILD_ENV: &str = "RYNX_SANDBOX_TEST_CHILD";

fn outcome<T>(result: std::io::Result<T>) -> &'static str {
    match result {
        Ok(_) => "allowed",
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => "denied",
        Err(_) => "error",
    }
}

/// Runs inside the sandboxed child: try each kind of access and print the results
fn child(downloads: &Path, secret: &Path, outside: &Path) {
    let status = restrict_filesystem(&[downloads]);
    println!("status={:?}", status);
    println!("read_secret={}", outcome(std::fs::read(secret)));
    println!("write_outside={}", outcome(std::fs::write(outside, b"x")));
    println!("write_download={}", outcome(std::fs::write(downloads.join("file.zip"), b"x")));
    println!("read_download={}", outcome(std::fs::read(downloads.join("file.zip"))));
    println!("read_etc={}", outcome(std::fs::read("/etc/hosts")));
    println!("open_tty={}", outcome(std::fs::OpenOptions::new().read(true).write(true).open("/dev/null")));
    // Threads started after the sandbox inherit it
    let secret = secret.to_path_buf();
    let in_thread = std::thread::spawn(move || outcome(std::fs::read(&secret))).join().unwrap();
    println!("spawned_thread_secret={}", in_thread);
}

#[test]
fn sandbox_allows_downloads_and_system_files_only() {
    let dirs = |name: &str| -> PathBuf { PathBuf::from(std::env::var(name).unwrap()) };
    if std::env::var(CHILD_ENV).is_ok() {
        return child(&dirs("SBX_DOWNLOADS"), &dirs("SBX_SECRET"), &dirs("SBX_OUTSIDE"));
    }

    // Stand-ins for the Downloads folder and for private files elsewhere (e.g. ~/.ssh)
    let root = std::env::temp_dir().join(format!("rynx-sandbox-test-{}", std::process::id()));
    let downloads = root.join("Downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    let secret = root.join("id_ed25519");
    std::fs::write(&secret, b"private key").unwrap();
    let outside = root.join("planted.sh");

    let output = Command::new(std::env::current_exe().unwrap())
        .args(["sandbox_allows_downloads_and_system_files_only", "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, "1")
        .env("SBX_DOWNLOADS", &downloads)
        .env("SBX_SECRET", &secret)
        .env("SBX_OUTSIDE", &outside)
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&output.stdout).to_string();
    let _ = std::fs::remove_dir_all(&root);
    let value = |key: &str| -> String {
        report
            .lines()
            // libtest may print "test ... " on the same line as the child's first output
            .find_map(|l| l.split(&format!("{key}=")).nth(1))
            .unwrap_or_else(|| panic!("no {key} in child output:\n{report}"))
            .to_string()
    };

    let status = value("status");
    if status == format!("{:?}", SandboxStatus::Unavailable) {
        eprintln!("skipping: this kernel has no Landlock support");
        return;
    }
    assert!(
        status == format!("{:?}", SandboxStatus::Enforced) || status == format!("{:?}", SandboxStatus::Partial),
        "sandbox not applied: {status}"
    );
    assert_eq!(value("read_secret"), "denied", "{report}");
    assert_eq!(value("write_outside"), "denied", "{report}");
    assert_eq!(value("write_download"), "allowed", "{report}");
    assert_eq!(value("read_download"), "allowed", "{report}");
    assert_eq!(value("read_etc"), "allowed", "{report}");
    assert_eq!(value("open_tty"), "allowed", "{report}");
    assert_eq!(value("spawned_thread_secret"), "denied", "{report}");
}

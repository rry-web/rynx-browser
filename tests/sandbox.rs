//! The filesystem sandbox (Landlock on Linux, Seatbelt on macOS). Restrictions can't be lifted
//! once applied, so the test re-runs itself in a child process that sandboxes itself and reports
//! what it could and couldn't do.
#![cfg(any(target_os = "linux", target_os = "macos"))]

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
    // No programs, and no listening; connections out still work
    println!("run_program={}", outcome(Command::new("/bin/echo").arg("hi").output()));
    let port: u16 = std::env::var("SBX_PORT").unwrap().parse().unwrap();
    println!("connect_out={}", outcome(std::net::TcpStream::connect(("127.0.0.1", port))));
    println!("listen={}", outcome(std::net::TcpListener::bind("127.0.0.1:0")));
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

    // Stand-ins for the home folder, its Downloads folder, and private files elsewhere in it (e.g.
    // ~/.ssh). On macOS the sandbox protects the home folder, so the child gets this one.
    let root = std::env::temp_dir().join(format!("rynx-sandbox-test-{}", std::process::id()));
    let downloads = root.join("Downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    let secret = root.join("id_ed25519");
    std::fs::write(&secret, b"private key").unwrap();
    let outside = root.join("planted.sh");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();

    let output = Command::new(std::env::current_exe().unwrap())
        .args(["sandbox_allows_downloads_and_system_files_only", "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, "1")
        .env("SBX_DOWNLOADS", &downloads)
        .env("SBX_SECRET", &secret)
        .env("SBX_OUTSIDE", &outside)
        .env("SBX_PORT", listener.local_addr().unwrap().port().to_string())
        .env("HOME", &root)
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
    assert_eq!(value("run_program"), "denied", "{report}");
    assert_eq!(value("connect_out"), "allowed", "{report}");
    // Older kernels (Landlock before ABI 4) can't restrict listening; the status says Partial
    if status != format!("{:?}", SandboxStatus::Partial) {
        assert_eq!(value("listen"), "denied", "{report}");
    }
}

/// Runs inside the sandboxed child, with the real home folder: what the browser does all day
#[cfg(target_os = "macos")]
fn working_child(downloads: &Path, config: &Path) {
    let status = restrict_filesystem(&[downloads, config]);
    println!("status={:?}", status);
    let home = PathBuf::from(std::env::var("HOME").unwrap());
    println!("read_home_file={}", outcome(std::fs::read_dir(&home)));
    println!("write_config={}", outcome(std::fs::write(config.join("bookmarks.txt"), b"x")));
    let file = downloads.join("page.html");
    println!("write_download={}", outcome(std::fs::write(&file, b"x")));
    // Download tagging (quarantine) writes extended attributes
    println!("xattr_download={}", outcome(xattr::set(&file, "com.apple.quarantine", b"0081;00000000;rynx;")));
    // HTTPS with the operating system's certificate checks, and DNS
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let https = runtime.block_on(async {
        let manager = rynx::network::NetworkManager::new().unwrap();
        let client = manager.get_client(false);
        match rynx::network::get_following_redirects(client, "https://example.com/", false).await {
            Ok(response) => format!("status {}", response.status().as_u16()),
            Err(e) => format!("error {e}"),
        }
    });
    println!("https={https}");
    let bad_cert = runtime.block_on(async {
        let manager = rynx::network::NetworkManager::new().unwrap();
        rynx::network::get_following_redirects(manager.get_client(false), "https://expired.badssl.com/", false)
            .await
            .is_err()
    });
    println!("bad_cert_rejected={bad_cert}");
    let clipboard = arboard::Clipboard::new()
        .and_then(|mut c| {
            c.set_text("rynx sandbox check")?;
            c.get_text()
        })
        .map(|text| (text == "rynx sandbox check").to_string())
        .unwrap_or_else(|e| format!("error {e}"));
    println!("clipboard={clipboard}");
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "needs the internet and the clipboard; run with --ignored"]
fn the_browser_works_inside_the_macos_sandbox() {
    if std::env::var(CHILD_ENV).is_ok() {
        let dirs = |name: &str| -> PathBuf { PathBuf::from(std::env::var(name).unwrap()) };
        return working_child(&dirs("SBX_DOWNLOADS"), &dirs("SBX_CONFIG"));
    }
    let root = std::env::temp_dir().join(format!("rynx-sandbox-works-{}", std::process::id()));
    let (downloads, config) = (root.join("Downloads"), root.join("config"));
    std::fs::create_dir_all(&downloads).unwrap();
    std::fs::create_dir_all(&config).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["the_browser_works_inside_the_macos_sandbox", "--exact", "--nocapture", "--ignored", "--test-threads=1"])
        .env(CHILD_ENV, "1")
        .env("SBX_DOWNLOADS", &downloads)
        .env("SBX_CONFIG", &config)
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&output.stdout).to_string();
    let _ = std::fs::remove_dir_all(&root);
    println!("{report}");
    eprintln!("{}", String::from_utf8_lossy(&output.stderr));
    let value =
        |key: &str| report.lines().find_map(|l| l.split(&format!("{key}=")).nth(1)).unwrap_or("missing").to_string();
    assert_eq!(value("status"), "Enforced", "{report}");
    assert_eq!(value("read_home_file"), "denied", "{report}");
    assert_eq!(value("write_config"), "allowed", "{report}");
    assert_eq!(value("write_download"), "allowed", "{report}");
    assert_eq!(value("xattr_download"), "allowed", "{report}");
    assert_eq!(value("https"), "status 200", "{report}");
    assert_eq!(value("bad_cert_rejected"), "true", "{report}");
    assert_eq!(value("clipboard"), "true", "{report}");
}

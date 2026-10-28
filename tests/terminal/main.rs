//! The real program, run in a pseudo-terminal: what's drawn is read back through a terminal
//! emulator (vt100), and keys are typed as a person would. Each test gets its own home folder,
//! so settings, cookies and downloads are its own. Unix only (Windows' pseudo-terminals differ,
//! and it has no sandbox to check).
#![cfg(unix)]

mod harness;
use harness::*;

use std::path::PathBuf;
use std::time::Duration;

mod browsing;
mod forms;
mod security;
mod settings;

#[test]
fn it_starts_sandboxed_shows_the_start_page_and_quits_cleanly() {
    let home = Home::new("start");
    home.settings("");
    let mut rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    // The sandbox says how it went (Linux kernels without Landlock say so too)
    rynx.wait_for("sandbox");
    assert!(rynx.text().contains("Clearweb"));
    rynx.keys("q");
    assert!(rynx.wait_exit(), "rynx exited with an error:\n{}", rynx.screen());
    assert!(rynx.left_terminal_as_found(), "the terminal was left in rynx's screen");
    // The first run writes the settings and keys files
    assert!(home.config().join("keys.conf").is_file());
}

#[test]
fn pages_open_links_are_followed_and_back_returns() {
    let site = Site::new(&[
        ("/", "<title>Home</title><h1>Welcome home</h1><p><a href=\"/second\">The second page</a></p>"),
        ("/second", "<title>Second</title><h1>Second page here</h1>"),
    ]);
    let home = Home::new("browse");
    home.settings("");
    let mut rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    rynx.open(&site.url("/"));
    rynx.wait_for("Welcome home");
    // Tab selects the link (its destination shows), Enter follows it
    rynx.keys("\t");
    rynx.wait_for("/second");
    rynx.keys("\r");
    rynx.wait_for("Second page here");
    rynx.keys("\u{7f}");
    rynx.wait_for("Welcome home");
    rynx.keys("q");
    assert!(rynx.wait_exit());
}

#[test]
fn keys_conf_changes_the_keys() {
    let home = Home::new("keys");
    home.settings("");
    std::fs::write(home.config().join("keys.conf"), "quit = Ctrl+q only\nsettings = F2\n").unwrap();
    let mut rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    // F2 (as xterm sends it) opens the settings
    rynx.keys("\x1bOQ");
    rynx.wait_for("Settings");
    rynx.wait_for("I2P router");
    // q no longer quits
    rynx.keys("q");
    std::thread::sleep(Duration::from_millis(500));
    assert!(rynx.child.try_wait().unwrap().is_none(), "q quit, though keys.conf took it away");
    // Ctrl+q does
    rynx.keys("\u{11}");
    assert!(rynx.wait_exit());
}

#[test]
fn a_mistake_in_keys_conf_is_named() {
    let home = Home::new("keys-mistake");
    home.settings("");
    std::fs::write(home.config().join("keys.conf"), "relaod = F5\n").unwrap();
    let mut rynx = Rynx::start(&home);
    rynx.wait_for("Problem in keys.conf, line 1: unknown action \"relaod\"");
    rynx.keys("q");
    assert!(rynx.wait_exit());
}

/// A stand-in for i2pd: writes its pid down, then waits to be stopped
fn fake_i2pd(home: &Home) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let program = home.0.join("i2pd");
    std::fs::write(&program, format!("#!/bin/sh\necho $$ > {}\nexec sleep 60\n", home.0.join("router.pid").display()))
        .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    program
}

fn router_pid(home: &Home) -> u32 {
    let file = home.0.join("router.pid");
    for _ in 0..50 {
        if let Some(pid) = std::fs::read_to_string(&file).ok().and_then(|t| t.trim().parse().ok()) {
            return pid;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("the router never started");
}

#[test]
fn the_router_starts_with_rynx_and_stops_when_it_quits() {
    let home = Home::new("router");
    let program = fake_i2pd(&home);
    home.settings(&format!(
        "i2p_router = start\ni2pd_path = {}\ni2p_proxy = http://127.0.0.1:{}\n",
        program.display(),
        free_port()
    ));
    let mut rynx = Rynx::start(&home);
    rynx.wait_for("Starting the I2P router");
    let pid = router_pid(&home);
    assert!(alive(pid));
    rynx.keys("S");
    rynx.wait_for(&format!("rynx started i2pd (process {pid})"));
    rynx.wait_for("stops when rynx quits");
    rynx.keys("q");
    assert!(rynx.wait_exit());
    std::thread::sleep(Duration::from_millis(300));
    assert!(!alive(pid), "the router was left running");
}

#[test]
fn a_router_kept_running_outlives_rynx() {
    let home = Home::new("router-keep");
    let program = fake_i2pd(&home);
    home.settings(&format!(
        "i2p_router = start\ni2p_router_keep = on\ni2pd_path = {}\ni2p_proxy = http://127.0.0.1:{}\n",
        program.display(),
        free_port()
    ));
    let mut rynx = Rynx::start(&home);
    rynx.wait_for("Starting the I2P router");
    let pid = router_pid(&home);
    rynx.keys("q");
    assert!(rynx.wait_exit());
    std::thread::sleep(Duration::from_millis(300));
    assert!(alive(pid), "a router kept running was stopped");
    let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
}

#[test]
fn a_router_already_running_is_left_alone() {
    let home = Home::new("router-other");
    let program = fake_i2pd(&home);
    let other = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    home.settings(&format!(
        "i2p_router = start\ni2pd_path = {}\ni2p_proxy = http://{}\n",
        program.display(),
        other.local_addr().unwrap()
    ));
    let mut rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    rynx.keys("S");
    rynx.wait_for("rynx left it alone");
    assert!(!home.0.join("router.pid").exists(), "a second router was started");
    rynx.keys("q");
    assert!(rynx.wait_exit());
}

#[test]
fn a_file_link_is_downloaded_after_asking() {
    let site = Site::new(&[("/", "<p><a href=\"/files/report.zip\">The report</a></p>")]);
    {
        use wiremock::matchers::path;
        use wiremock::{Mock, ResponseTemplate};
        site.runtime.block_on(
            Mock::given(path("/files/report.zip"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_raw(b"PK\x03\x04 zip bytes".to_vec(), "application/zip"),
                )
                .mount(&site.server),
        );
    }
    let home = Home::new("download");
    home.settings("");
    let mut rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    rynx.open(&site.url("/"));
    rynx.wait_for("The report");
    rynx.keys("\t");
    rynx.keys("\r");
    // A file that can't be shown as a page: it says so, and d saves it, after asking
    rynx.wait_for("can't be shown as a page");
    rynx.keys("d");
    rynx.wait_for("report.zip");
    rynx.keys("y");
    let file = home.0.join("Downloads/report.zip");
    for _ in 0..100 {
        if std::fs::read(&file).is_ok_and(|d| d.starts_with(b"PK")) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let downloaded =
        std::fs::read(&file).unwrap_or_else(|e| panic!("not downloaded ({e}). The screen:\n{}", rynx.screen()));
    assert_eq!(downloaded, b"PK\x03\x04 zip bytes");
    rynx.keys("q");
    assert!(rynx.wait_exit());
}

#[test]
fn a_site_allowed_cookies_keeps_you_signed_in_across_restarts() {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, ResponseTemplate};
    let site = Site::new(&[(
        "/login",
        "<form method=post action=/login><input name=user value=me><input type=password name=pw value=x><button>Sign in</button></form>",
    )]);
    site.runtime.block_on(async {
        Mock::given(method("POST"))
            .and(path("/login"))
            .respond_with(
                ResponseTemplate::new(303)
                    .insert_header("location", "/account")
                    .append_header("set-cookie", "session=s3cret; Path=/; Max-Age=86400"),
            )
            .mount(&site.server)
            .await;
        Mock::given(path("/account"))
            .and(header("cookie", "session=s3cret"))
            .respond_with(ResponseTemplate::new(200).set_body_raw("<h1>Welcome back, me</h1>", "text/html"))
            .mount(&site.server)
            .await;
        Mock::given(path("/account"))
            .respond_with(ResponseTemplate::new(200).set_body_raw("<h1>Please sign in</h1>", "text/html"))
            .mount(&site.server)
            .await;
    });
    let home = Home::new("cookies");
    home.settings("");
    let mut rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    rynx.open(&site.url("/login"));
    rynx.wait_for("press C before signing in");
    // The form as drawn: the fields' values, and the button's whole label
    rynx.wait_for("[me__________________][•___________________][Sign in]");
    rynx.keys("C");
    rynx.wait_for("may keep cookies");
    // The form's button: the first field is selected already; Tab past the second to it
    rynx.keys("\t\t");
    rynx.keys("\r");
    rynx.wait_for("Welcome back, me");
    rynx.keys("q");
    assert!(rynx.wait_exit());

    // Started again: still signed in
    let mut again = Rynx::start(&home);
    again.wait_for("RYNX BROWSER");
    again.open(&site.url("/account"));
    again.wait_for("Welcome back, me");
    again.open("about:cookies");
    again.wait_for("127.0.0.1");
    again.keys("q");
    assert!(again.wait_exit());
}

/// The whole update, as a person does it: the daily check finds a release on (a fake) GitHub,
/// U shows it, its button downloads and checks it, and the next start installs it and starts it
#[test]
fn an_update_is_found_downloaded_and_installed_at_the_next_start() {
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use wiremock::matchers::path;
    use wiremock::{Mock, ResponseTemplate};
    let home = Home::new("update");
    home.settings("updates = daily\n");
    // A copy outside target/, which rynx can update (a development build there can't)
    let program = home.0.join("bin/rynx");
    std::fs::create_dir_all(program.parent().unwrap()).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_rynx"), &program).unwrap();

    let key =
        Ed25519KeyPair::from_pkcs8(Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap().as_ref())
            .unwrap();
    let new_program = "#!/bin/sh\necho \"the new rynx, $RYNX_UPDATED\"\nsleep 5\n".to_string();
    let name = format!("rynx-99.0.0-{}", rynx::update::TARGET);
    let digest: String = ring::digest::digest(&ring::digest::SHA256, new_program.as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();
    let sums = format!("{}  {}\n", digest, name);
    let signature = key.sign(sums.as_bytes()).as_ref().to_vec();
    let site = Site::new(&[]);
    let base = site.url("");
    let releases = serde_json::json!([{
        "tag_name": "v99.0.0", "prerelease": false, "draft": false, "body": "Everything is better.",
        "html_url": format!("{base}/release"),
        "assets": [
            { "name": name, "browser_download_url": format!("{base}/program") },
            { "name": "SHA256SUMS", "browser_download_url": format!("{base}/sums") },
            { "name": "SHA256SUMS.sig", "browser_download_url": format!("{base}/sig") },
        ]
    }]);
    site.runtime.block_on(async {
        for (at, body) in [
            ("/releases", releases.to_string().into_bytes()),
            ("/program", new_program.clone().into_bytes()),
            ("/sums", sums.clone().into_bytes()),
            ("/sig", signature.clone()),
        ] {
            Mock::given(path(at))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
                .mount(&site.server)
                .await;
        }
    });
    let public: String = key.public_key().as_ref().iter().map(|b| format!("{:02x}", b)).collect();
    let env = [("RYNX_TEST_UPDATE_API", format!("{base}/releases")), ("RYNX_TEST_RELEASE_KEY", public)];

    let mut rynx = Rynx::start_with(&program, &home, &[], &env);
    rynx.wait_for("rynx 99.0.0 is available");
    rynx.keys("U");
    rynx.wait_for("Everything is better.");
    // The first control on the page is the download button
    rynx.keys("\r");
    rynx.wait_for("downloaded and checked");
    rynx.keys("q");
    assert!(rynx.wait_exit());

    // The next start installs it before anything else, and starts it
    let next = Rynx::start_with(&program, &home, &[], &env);
    next.wait_for("the new rynx, 99.0.0");
    assert_eq!(std::fs::read_to_string(&program).unwrap(), new_program);
    drop(next);
}

use rynx::i2pd::{Plan, Progress, RouterState, local_proxy, parse_console, start_if_wanted};
use rynx::settings::{NetworkMode, Settings};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rynx-i2pd-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A stand-in for i2pd: writes its arguments down, then waits to be stopped
#[cfg(unix)]
fn fake_i2pd(dir: &Path, script: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let program = dir.join("i2pd");
    std::fs::write(&program, format!("#!/bin/sh\n{}\n", script)).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    program
}

/// Settings asking for the router (or not), with the proxy and i2pd given
fn router_settings(start: bool, keep: bool, proxy: &str, program: Option<&Path>) -> Settings {
    Settings {
        start_i2p_router: start,
        keep_i2p_router: keep,
        i2p_proxy: proxy.to_string(),
        i2pd_path: program.map(Path::to_path_buf),
        ..Settings::default()
    }
}

#[cfg(unix)]
fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// A free port on 127.0.0.1
/// An address nothing listens on (tests never open port 1), for a router that isn't running.
/// A "free" port wouldn't do: another test running at the same time could take it meanwhile.
const NOTHING_LISTENING: &str = "http://127.0.0.1:1";

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// From i2pd 3.0.0's console, two minutes after starting
const CONSOLE: &str = "<div class=\"content\"><b>Uptime:</b> 1 minute, 57 seconds<br>\n\
<b>Network status:</b> Firewalled<br>\n\
<b>Tunnel creation success rate:</b> 36%<br>\n\
<b>Routers:</b> 104&nbsp;&nbsp;&nbsp;<b>Floodfills:</b> 47&nbsp;&nbsp;&nbsp;<b>LeaseSets:</b> 0<br>\n\
<b>Client Tunnels:</b> 24&nbsp;&nbsp;&nbsp;<b>Transit Tunnels:</b> 0<br>\n";

#[test]
fn the_console_says_how_far_the_router_has_got() {
    let progress = parse_console(CONSOLE).expect("parsed");
    assert_eq!(
        progress,
        Progress {
            uptime: "1 minute, 57 seconds".to_string(),
            network: "Firewalled".to_string(),
            routers: 104,
            client_tunnels: 24,
            success_rate: Some(36),
        }
    );
    assert!(progress.ready());
    assert!(progress.describe().starts_with("ready: 24 tunnels, 104 routers known"), "{}", progress.describe());

    let starting =
        parse_console("<b>Uptime:</b> 3 seconds<br><b>Routers:</b> 0&nbsp;<b>Client Tunnels:</b> 0<br>").unwrap();
    assert!(!starting.ready());
    assert!(starting.describe().starts_with("joining the network"), "{}", starting.describe());
    let building =
        parse_console("<b>Uptime:</b> 9 seconds<br><b>Routers:</b> 80&nbsp;<b>Client Tunnels:</b> 1<br>").unwrap();
    assert!(building.describe().starts_with("building tunnels (1 so far, 80 routers known)"));
    // Not a console
    assert_eq!(parse_console("<html>Hello</html>"), None);
    assert_eq!(parse_console("<b>Uptime:</b>"), Some(Progress::default()));
}

#[test]
fn only_a_proxy_on_this_computer_can_have_its_router_started() {
    assert_eq!(local_proxy("http://127.0.0.1:4444"), Some(SocketAddr::from(([127, 0, 0, 1], 4444))));
    assert!(local_proxy("http://localhost:4444").is_some_and(|a| a.ip().is_loopback()));
    assert_eq!(local_proxy("http://[::1]:4444").map(|a| a.port()), Some(4444));
    assert_eq!(local_proxy("http://192.168.1.20:4444"), None);
    assert_eq!(local_proxy("not an address"), None);
}

#[test]
fn the_router_gets_only_what_rynx_needs() {
    let plan = Plan {
        program: PathBuf::from("/usr/bin/i2pd"),
        data_dir: PathBuf::from("/data/i2pd"),
        certificates: Some(PathBuf::from("/usr/share/i2pd/certificates")),
        proxy: SocketAddr::from(([127, 0, 0, 1], 4444)),
        console_port: Some(7070),
        sam_port: None,
        keep: false,
    };
    let args = plan.args();
    // Paths as this system joins them
    let datadir = format!("--datadir={}", Path::new("/data/i2pd").display());
    let conf = format!("--conf={}", Path::new("/data/i2pd").join("i2pd.conf").display());
    for expected in [
        datadir.as_str(),
        conf.as_str(),
        "--httpproxy.address=127.0.0.1",
        "--httpproxy.port=4444",
        "--socksproxy.enabled=0",
        "--sam.enabled=0",
        "--http.address=127.0.0.1",
        "--http.port=7070",
        "--certsdir=/usr/share/i2pd/certificates",
        // One that stops when rynx quits carries no one else's traffic
        "--notransit",
    ] {
        assert!(args.iter().any(|a| a == expected), "{expected} missing from {args:?}");
    }
    let kept = Plan { keep: true, console_port: None, certificates: None, ..plan.clone() }.args();
    assert!(!kept.iter().any(|a| a == "--notransit"));
    assert!(kept.iter().any(|a| a == "--http.enabled=0"));
    assert!(!kept.iter().any(|a| a.starts_with("--certsdir")));
    // SAM, when asked for, listens on this computer only
    let sam = Plan { sam_port: Some(7656), ..plan }.args();
    for expected in ["--sam.enabled=1", "--sam.address=127.0.0.1", "--sam.port=7656"] {
        assert!(sam.iter().any(|a| a == expected), "{expected} missing from {sam:?}");
    }
    assert!(!sam.iter().any(|a| a == "--sam.enabled=0"));
}

#[test]
fn nothing_is_started_unless_it_should_be() {
    let dir = temp_dir("decide");
    let proxy = NOTHING_LISTENING.to_string();
    let missing = dir.join("no-such-i2pd");
    let decide = |wanted, clearweb: bool, proxy: &str| {
        let mut settings = router_settings(wanted, false, proxy, Some(&missing));
        if clearweb {
            settings.network = NetworkMode::Clearweb;
        }
        start_if_wanted(&settings, Some(dir.clone())).0
    };
    assert_eq!(decide(false, false, &proxy), RouterState::NotManaged);
    assert_eq!(decide(true, true, &proxy), RouterState::Clearweb);
    assert_eq!(decide(true, false, "http://192.168.1.20:4444"), RouterState::RemoteProxy);
    assert_eq!(decide(true, false, &proxy), RouterState::NotFound);
    // A router already at the proxy's address is left alone
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let running = format!("http://{}", listener.local_addr().unwrap());
    assert_eq!(decide(true, false, &running), RouterState::AlreadyRunning);
    assert!(RouterState::NotFound.startup_note().unwrap().contains("isn't installed"));
    assert_eq!(RouterState::AlreadyRunning.startup_note(), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn certificates_are_found_next_to_the_program() {
    let dir = temp_dir("certs");
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    std::fs::create_dir_all(dir.join("share/i2pd/certificates")).unwrap();
    std::fs::write(dir.join("bin/i2pd"), "").unwrap();
    assert_eq!(rynx::i2pd::certificates_for(&dir.join("bin/i2pd")), Some(dir.join("share/i2pd/certificates")));
    assert_eq!(rynx::i2pd::find(Some(&dir.join("bin/i2pd"))), Some(dir.join("bin/i2pd")));
    assert_eq!(rynx::i2pd::find(Some(&dir.join("bin/missing"))), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_router_rynx_starts_is_stopped_when_rynx_quits() {
    let dir = temp_dir("start");
    let program = fake_i2pd(&dir, "echo \"$@\" > \"$(dirname \"$0\")/args.txt\"\nexec sleep 30");
    let data = dir.join("data");
    let proxy = NOTHING_LISTENING.to_string();
    let mut settings = router_settings(true, false, &proxy, Some(&program));
    settings.i2p_router_sam = true;
    let (state, router) = start_if_wanted(&settings, Some(data.clone()));
    let RouterState::Started { pid, keep: false, sam_port, .. } = state else { panic!("{state:?}") };
    let router = router.expect("a router");
    assert!(alive(pid));
    // Empty configuration files for it, never a hand-made i2pd's
    assert!(data.join("i2pd.conf").is_file() && data.join("tunnels.conf").is_file());
    let args = std::fs::read_to_string(dir.join("args.txt")).unwrap();
    assert!(args.contains(&format!("--datadir={}", data.display())), "{args}");
    assert!(args.contains("--notransit"));
    // SAM was asked for: on at its usual port, unless something here already has that port
    if rynx::i2pd::listening(SocketAddr::from(([127, 0, 0, 1], rynx::i2pd::SAM_PORT))) && sam_port.is_none() {
        assert!(args.contains("--sam.enabled=0"), "{args}");
    } else {
        assert_eq!(sam_port, Some(rynx::i2pd::SAM_PORT));
        assert!(args.contains("--sam.enabled=1") && args.contains("--sam.address=127.0.0.1"), "{args}");
    }
    router.stop();
    assert!(!alive(pid), "still running after rynx quit");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_router_kept_running_outlives_rynx() {
    let dir = temp_dir("keep");
    let program = fake_i2pd(&dir, "exec sleep 30");
    let proxy = NOTHING_LISTENING.to_string();
    let (state, router) = start_if_wanted(&router_settings(true, true, &proxy, Some(&program)), Some(dir.join("data")));
    let RouterState::Started { pid, keep: true, .. } = state else { panic!("{state:?}") };
    router.unwrap().stop();
    assert!(alive(pid), "a router kept running was stopped");
    let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_router_that_wont_start_says_why() {
    let dir = temp_dir("fail");
    let program = fake_i2pd(&dir, "echo 'missing/unreadable config file: x' >&2\nexit 1");
    let proxy = NOTHING_LISTENING.to_string();
    let (state, router) =
        start_if_wanted(&router_settings(true, false, &proxy, Some(&program)), Some(dir.join("data")));
    assert!(router.is_none());
    assert_eq!(state, RouterState::Failed("missing/unreadable config file: x".to_string()));
    assert!(state.startup_note().unwrap().contains("didn't start: missing/unreadable"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A router an earlier rynx started and didn't stop (it crashed) is recognised by its pid file
/// and the program its process runs, so a process that merely has its old pid isn't touched.
/// This test's own process stands in for the router (kept running, so nothing is signalled).
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn a_router_left_running_by_a_crash_is_recognised() {
    let dir = temp_dir("left");
    let this = std::env::current_exe().unwrap();
    std::fs::write(dir.join("i2pd.pid"), std::process::id().to_string()).unwrap();
    let router = rynx::i2pd::Router::left_running(&dir, &this, true).expect("recognised");
    assert_eq!(router.pid(), std::process::id());
    router.stop();

    // Another program at that pid isn't the router
    let other = dir.join("other-program");
    std::fs::write(&other, "").unwrap();
    assert!(rynx::i2pd::Router::left_running(&dir, &other, true).is_none());
    // Nor is a pid with no process
    std::fs::write(dir.join("i2pd.pid"), "999999999").unwrap();
    assert!(rynx::i2pd::Router::left_running(&dir, &this, true).is_none());
    std::fs::write(dir.join("i2pd.pid"), "not a pid").unwrap();
    assert!(rynx::i2pd::Router::left_running(&dir, &this, true).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_router_settings_are_saved_and_read_back() {
    let (settings, problems) =
        rynx::settings::Settings::parse("i2p_router = start\ni2p_router_keep = on\ni2pd_path = /opt/i2pd/i2pd");
    assert!(problems.is_empty(), "{problems:?}");
    assert!(settings.start_i2p_router && settings.keep_i2p_router);
    assert_eq!(settings.i2pd_path, Some(PathBuf::from("/opt/i2pd/i2pd")));
    let (again, _) = rynx::settings::Settings::parse(&settings.to_file_text());
    assert_eq!(again, settings);
    let defaults = rynx::settings::Settings::default();
    assert!(!defaults.start_i2p_router && !defaults.keep_i2p_router);
    let (_, problems) = rynx::settings::Settings::parse("i2p_router = yes");
    assert_eq!(problems, vec!["line 1: invalid i2p_router \"yes\""]);
}

#[tokio::test]
async fn the_settings_page_shows_the_router() {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = rynx::app::App::new(tx, rx).unwrap();
    app.render_in_background = false;
    app.router_state = RouterState::Started { pid: 4242, console_port: Some(7070), sam_port: Some(7656), keep: false };
    app.finish_router_progress(parse_console(CONSOLE));
    assert!(app.current_tab().status_message.starts_with("The I2P router is ready"));
    // Said once, not every time the console is asked
    app.current_tab().status_message = String::new();
    app.finish_router_progress(parse_console(CONSOLE));
    assert_eq!(app.current_tab().status_message, "");

    app.follow_link(rynx::app::SETTINGS_PAGE.to_string());
    let tab = app.current_tab();
    let text: String = tab.rendered_content.iter().map(|l| l.to_string()).collect::<Vec<_>>().join(" ");
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(text.contains("rynx started i2pd (process 4242): ready: 24 tunnels"), "{text}");
    assert!(text.contains("It stops when rynx quits"), "{text}");
    assert!(text.contains("Other programs can use it through SAM at 127.0.0.1:7656"), "{text}");
    for name in ["i2p_router", "i2p_router_keep", "i2p_router_sam", "i2pd_path"] {
        assert!(tab.controls.iter().any(|c| c.name == name), "no {name} on the page");
    }
}

/// With the real i2pd (installed with a package manager): it starts with rynx's options, its
/// console answers, and it stops. `cargo test --test i2pd -- --ignored`
#[cfg(unix)]
#[tokio::test]
#[ignore]
async fn the_real_i2pd_starts_reports_and_stops() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    // Where rynx keeps it, so a distribution's AppArmor profile for i2pd (which allows ~/.i2pd
    // on Linux) lets it write there
    let dir = rynx::i2pd::data_dir().unwrap().with_file_name(format!("rynx-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let port = free_port();
    let mut settings = router_settings(true, false, &format!("http://127.0.0.1:{}", port), None);
    settings.i2p_router_sam = true;
    let (state, router) = start_if_wanted(&settings, Some(dir.clone()));
    // The console and SAM are off when something here already has their ports (a router of
    // your own, say)
    let RouterState::Started { pid, console_port, sam_port, .. } = state else { panic!("{state:?}") };
    // It runs, with rynx's options, and opens its proxy
    let start = std::time::Instant::now();
    while !rynx::i2pd::listening(SocketAddr::from(([127, 0, 0, 1], port))) {
        assert!(alive(pid), "it stopped: {}", rynx::i2pd::log_tail(&dir, 10).join("\n"));
        assert!(start.elapsed() < std::time::Duration::from_secs(60), "the proxy never opened");
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    // SAM, asked for, opens on this computer
    if let Some(sam) = sam_port {
        let start = std::time::Instant::now();
        while !rynx::i2pd::listening(SocketAddr::from(([127, 0, 0, 1], sam))) {
            assert!(start.elapsed() < std::time::Duration::from_secs(60), "SAM never opened");
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }
    // Its console says how far it's got. Newer i2pd answers within a second; 2.49 (Ubuntu
    // 24.04's) didn't answer within two minutes of starting on CI, which rynx shows as
    // "starting", so the console is only reported here, not required
    if let Some(console) = console_port {
        let mut progress = None;
        let start = std::time::Instant::now();
        while progress.is_none() && start.elapsed() < std::time::Duration::from_secs(30) {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            progress = rynx::i2pd::progress(console).await;
        }
        match &progress {
            Some(progress) => println!("console: {}", progress.describe()),
            None => println!("console: no answer within 30 s (older i2pd versions are slow to answer while starting)"),
        }
    }
    router.unwrap().stop();
    assert!(!alive(pid));
    let _ = std::fs::remove_dir_all(&dir);
}

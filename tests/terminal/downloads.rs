//! Downloads, and what the system is told about them (DL, PLAT-03/05/06/07 in TESTING.md)

use super::harness::*;
use std::time::Duration;

fn started(name: &str) -> (Home, Rynx) {
    let home = Home::new(name);
    home.settings("");
    let rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    (home, rynx)
}

fn file(site: &Site, at: &str, body: Vec<u8>, kind: &str) {
    site.respond(at, wiremock::ResponseTemplate::new(200).set_body_raw(body, kind));
}

/// Open a page of links, select the one with `text` (Tab until it's named) and press d, then y
fn download(rynx: &mut Rynx, page: &str, link: &str) {
    rynx.open(page);
    rynx.wait_for("Downloads test");
    for _ in 0..10 {
        if rynx.text().contains(&format!("Link: {link}")) {
            break;
        }
        rynx.keys("\t");
    }
    rynx.wait_for(&format!("Link: {link}"));
    rynx.keys("d");
}

/// DL-01, DL-02: a big file downloads at local speed, named from its address without the query
#[test]
fn dl_01_02_speed_filename_and_query_strings() {
    let site = Site::new(&[]);
    let big: Vec<u8> = (0..20 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    file(&site, "/big/archive.zip", big.clone(), "application/zip");
    let links = format!("<h1>Downloads test</h1><p><a href=\"{}\">big</a></p>", site.url("/big/archive.zip?token=abc"));
    site.page("/", &links);
    let (home, mut rynx) = started("big-download");
    download(&mut rynx, &site.url("/"), &site.url("/big/archive.zip?token=abc"));
    rynx.wait_for("archive.zip");
    let start = std::time::Instant::now();
    rynx.keys("y");
    home.wait_for_file(&home.downloads().join("archive.zip"), &big);
    eprintln!("20 MB downloaded in {:?}", start.elapsed());
    assert!(start.elapsed() < Duration::from_secs(15));
    rynx.quit();
}

/// DL-03: an error page isn't saved, and the page you're on stays
#[test]
fn dl_03_error_pages_arent_saved() {
    let site = Site::new(&[]);
    site.respond("/missing.zip", wiremock::ResponseTemplate::new(404).set_body_raw("<h1>Not found</h1>", "text/html"));
    site.page("/", &format!("<h1>Downloads test</h1><p><a href=\"{}\">gone</a></p>", site.url("/missing.zip")));
    let (home, mut rynx) = started("download-404");
    download(&mut rynx, &site.url("/"), &site.url("/missing.zip"));
    rynx.wait_for("missing.zip");
    rynx.keys("y");
    rynx.wait_for("404");
    assert!(rynx.text().contains("Downloads test"), "the page went away");
    assert!(std::fs::read_dir(home.downloads()).unwrap().next().is_none(), "something was saved");
    rynx.quit();
}

/// DL-04: a file is never silently overwritten
#[test]
fn dl_04_no_silent_overwrite() {
    let site = Site::new(&[]);
    file(&site, "/photo", b"\x89PNG\r\n\x1a\nnew picture".to_vec(), "image/png");
    site.page("/", &format!("<h1>Downloads test</h1><p><a href=\"{}\">photo</a></p>", site.url("/photo")));
    let (home, mut rynx) = started("no-overwrite");
    std::fs::write(home.downloads().join("photo.png"), b"the old one").unwrap();
    download(&mut rynx, &site.url("/"), &site.url("/photo"));
    rynx.wait_for("photo");
    rynx.keys("y");
    home.wait_for_file(&home.downloads().join("photo_1.png"), b"\x89PNG\r\n\x1a\nnew picture");
    assert_eq!(std::fs::read(home.downloads().join("photo.png")).unwrap(), b"the old one");
    rynx.quit();
}

/// DL-05: one download at a time per tab
#[test]
fn dl_05_one_download_per_tab() {
    let site = Site::new(&[]);
    site.respond(
        "/slow.zip",
        wiremock::ResponseTemplate::new(200)
            .set_delay(Duration::from_secs(5))
            .set_body_raw(b"PK slow".to_vec(), "application/zip"),
    );
    file(&site, "/other.zip", b"PK other".to_vec(), "application/zip");
    site.page(
        "/",
        &format!(
            "<h1>Downloads test</h1><p><a href=\"{}\">slow</a></p><p><a href=\"{}\">other</a></p>",
            site.url("/slow.zip"),
            site.url("/other.zip")
        ),
    );
    let (_home, mut rynx) = started("one-download");
    download(&mut rynx, &site.url("/"), &site.url("/slow.zip"));
    rynx.wait_for("slow.zip");
    rynx.keys("y");
    rynx.keys("\t");
    rynx.wait_for(&format!("Link: {}", site.url("/other.zip")));
    rynx.keys("d");
    rynx.wait_for("A download is already in progress in this tab");
    rynx.quit();
}

/// PLAT-06: a file named like a Windows device is saved under another name
#[test]
fn plat_06_device_names() {
    let site = Site::new(&[]);
    file(&site, "/con.txt", b"not a device".to_vec(), "application/octet-stream");
    site.page("/", &format!("<h1>Downloads test</h1><p><a href=\"{}\">con</a></p>", site.url("/con.txt")));
    let (home, mut rynx) = started("device-names");
    download(&mut rynx, &site.url("/"), &site.url("/con.txt"));
    rynx.wait_for("con.txt");
    rynx.keys("y");
    home.wait_for_file(&home.downloads().join("_con.txt"), b"not a device");
    rynx.quit();
}

/// DL-06, PLAT-03, PLAT-05: a download is tagged with where it came from, so the system checks
/// it before it's opened (macOS quarantine and "Where from"; Linux user.xdg.origin.url)
#[test]
fn dl_06_plat_03_05_downloads_are_tagged() {
    let site = Site::new(&[]);
    file(&site, "/tool.zip", b"PK tool".to_vec(), "application/zip");
    site.page("/", &format!("<h1>Downloads test</h1><p><a href=\"{}\">tool</a></p>", site.url("/tool.zip")));
    let (home, mut rynx) = started("tagged");
    download(&mut rynx, &site.url("/"), &site.url("/tool.zip"));
    rynx.wait_for("tool.zip");
    rynx.keys("y");
    let path = home.downloads().join("tool.zip");
    home.wait_for_file(&path, b"PK tool");
    std::thread::sleep(Duration::from_millis(300));
    let attribute =
        |name: &str| xattr::get(&path, name).ok().flatten().map(|v| String::from_utf8_lossy(&v).into_owned());
    if cfg!(target_os = "macos") {
        let quarantine = attribute("com.apple.quarantine").expect("no quarantine attribute");
        assert!(quarantine.contains(";rynx;"), "{quarantine}");
        let wherefrom =
            xattr::get(&path, "com.apple.metadata:kMDItemWhereFroms").ok().flatten().expect("no Where from");
        assert!(String::from_utf8_lossy(&wherefrom).contains("tool.zip"), "{wherefrom:?}");
    } else {
        let origin = attribute("user.xdg.origin.url").expect("no user.xdg.origin.url");
        assert_eq!(origin, site.url("/tool.zip"));
    }
    rynx.quit();
}

/// PLAT-07: --version, --help, and an unknown option
#[test]
fn plat_07_command_line_options() {
    let program = env!("CARGO_BIN_EXE_rynx");
    let run = |arg: &str| std::process::Command::new(program).arg(arg).output().unwrap();
    let version = run("--version");
    assert!(version.status.success());
    assert_eq!(String::from_utf8_lossy(&version.stdout).trim(), format!("rynx {}", env!("CARGO_PKG_VERSION")));
    let help = run("--help");
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--no-sandbox"));
    let unknown = run("--frobnicate");
    assert!(!unknown.status.success());
    let error = String::from_utf8_lossy(&unknown.stderr);
    assert!(error.contains("unknown option '--frobnicate'") && error.contains("--no-sandbox"), "{error}");
}

/// --no-sandbox says so
#[test]
fn plat_no_sandbox_is_reported() {
    let home = Home::new("no-sandbox");
    home.settings("");
    let mut rynx = Rynx::start_args(&home, &["--no-sandbox"]);
    rynx.wait_for("filesystem sandbox disabled (--no-sandbox)");
    rynx.quit();
}

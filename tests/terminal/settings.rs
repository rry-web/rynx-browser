//! Settings, bookmarks, history, copy mode, keys, updates (SET, KEY, UPD in TESTING.md)

use super::harness::*;
use std::time::Duration;

fn started_with(name: &str, settings: &str) -> (Home, Rynx) {
    let home = Home::new(name);
    home.settings(settings);
    let rynx = Rynx::start(&home);
    (home, rynx)
}

/// SET-01: the first run writes settings.conf with every option commented out
#[test]
fn set_01_settings_template() {
    let home = Home::new("template");
    let mut rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    rynx.quit();
    let text = home.read_config("settings.conf").expect("settings.conf written");
    let options: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).collect();
    assert!(options.is_empty(), "options set in the template: {options:?}");
    for key in [
        "home_page",
        "network",
        "search_url",
        "i2p_proxy",
        "history",
        "cache",
        "images",
        "updates",
        "i2p_router",
        "local_names",
    ] {
        assert!(text.contains(&format!("# {key} = ")), "{key} isn't in the template");
    }
}

/// SET-02: words typed in the address bar go to the search engine
#[test]
fn set_02_search_engine() {
    let site = Site::new(&[]);
    site.mount(
        wiremock::Mock::given(wiremock::matchers::path("/search"))
            .and(wiremock::matchers::query_param("q", "rust terminal"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_raw("<h1>Search results here</h1>", "text/html"),
            ),
    );
    let (_home, mut rynx) = started_with("search", &format!("search_url = {}\n", site.url("/search?q={query}")));
    rynx.wait_for("RYNX BROWSER");
    rynx.open("rust terminal");
    rynx.wait_for("Search results here");
    rynx.quit();
}

/// SET-03: the home page opens at the start and in new tabs
#[test]
fn set_03_home_page() {
    let site = Site::new(&[("/home", "<title>Home</title><h1>My home page</h1>")]);
    let (_home, mut rynx) = started_with("home-page", &format!("home_page = {}\n", site.url("/home")));
    rynx.wait_for("My home page");
    rynx.keys("n");
    rynx.wait_for("Home");
    assert_eq!(rynx.rows()[1].matches("Home").count(), 2, "{}", rynx.rows()[1]);
    rynx.quit();
}

/// SET-04: a bad setting is named; everything else works
#[test]
fn set_04_bad_settings() {
    let (_home, mut rynx) = started_with("bad-settings", "colour = blue\n");
    rynx.wait_for("Problem in settings.conf, line");
    rynx.wait_for("unknown setting \"colour\"");
    rynx.keys("S");
    rynx.wait_for("Settings");
    rynx.quit();
}

/// SET-05: a bookmark is listed, opens, survives a restart, and b again removes it
#[test]
fn set_05_bookmarks() {
    let site = Site::new(&[("/", "<title>Kept page</title><h1>Worth keeping</h1>")]);
    let (home, mut rynx) = started_with("bookmarks", "");
    rynx.wait_for("RYNX BROWSER");
    rynx.open(&site.url("/"));
    rynx.wait_for("Worth keeping");
    rynx.keys("b");
    rynx.keys("B");
    rynx.wait_for("Kept page");
    rynx.click_text("Kept page");
    rynx.wait_for("Worth keeping");
    rynx.quit();

    let mut again = Rynx::start(&home);
    again.wait_for("RYNX BROWSER");
    again.keys("B");
    again.wait_for("Kept page");
    again.click_text("Kept page");
    again.wait_for("Worth keeping");
    again.keys("b");
    again.keys("B");
    again.wait_gone("Kept page");
    again.quit();
    assert!(!home.read_config("bookmarks.txt").unwrap_or_default().contains("127.0.0.1"));
}

fn visit_three(rynx: &mut Rynx, site: &Site) {
    for name in ["one", "two", "three"] {
        rynx.open(&site.url(&format!("/{name}")));
        rynx.wait_for(&format!("Page {name} here"));
    }
}

fn three_pages() -> Site {
    Site::new(&[
        ("/one", "<title>First</title><h1>Page one here</h1>"),
        ("/two", "<title>Second</title><h1>Page two here</h1>"),
        ("/three", "<title>Third</title><h1>Page three here</h1>"),
    ])
}

/// SET-07: history is kept until you quit, newest first, and no file is written
#[test]
fn set_07_session_history() {
    let site = three_pages();
    let (home, mut rynx) = started_with("session-history", "");
    rynx.wait_for("RYNX BROWSER");
    visit_three(&mut rynx, &site);
    rynx.keys("s");
    rynx.wait_for("Third");
    let text = rynx.text();
    let (first, third) = (text.find("First").unwrap(), text.find("Third").unwrap());
    assert!(third < first, "not newest first:\n{}", rynx.screen());
    rynx.click_text("Second");
    rynx.wait_for("Page two here");
    rynx.quit();
    assert!(home.read_config("history.txt").is_none(), "history.txt was written");
    let mut again = Rynx::start(&home);
    again.wait_for("RYNX BROWSER");
    again.keys("s");
    again.wait_for("History");
    assert!(!again.text().contains("Second"), "history outlived the session");
    again.quit();
}

/// SET-08, SET-09: saved history survives a restart; x twice clears it, another key cancels
#[test]
fn set_08_09_saved_and_cleared_history() {
    let site = three_pages();
    let (home, mut rynx) = started_with("saved-history", "history = on\n");
    rynx.wait_for("RYNX BROWSER");
    visit_three(&mut rynx, &site);
    rynx.quit();
    let mut again = Rynx::start(&home);
    again.wait_for("RYNX BROWSER");
    again.keys("s");
    again.wait_for("Third");
    again.keys("x");
    again.wait_for("again");
    again.keys("j");
    again.keys("x");
    again.wait_for("again");
    again.keys("x");
    again.wait_gone("Third");
    again.quit();
    let file = home.read_config("history.txt").unwrap_or_default();
    assert!(!file.contains("127.0.0.1"), "history.txt still has pages: {file}");
}

/// SET-13: copy mode says so, counts the selection, and explains its keys
#[test]
fn set_13_copy_mode() {
    let site = Site::new(&[("/", "<p>Some text to select and copy here.</p>")]);
    let (_home, mut rynx) = started_with("copy-mode", "");
    rynx.wait_for("RYNX BROWSER");
    rynx.open(&site.url("/"));
    rynx.wait_for("Some text to select");
    rynx.keys("v");
    rynx.wait_for("COPY MODE");
    rynx.keys("llll");
    rynx.wait_for("COPY MODE 4 characters selected");
    rynx.wait_for("copies");
    // y would write the real clipboard; Esc leaves, and the address comes back
    rynx.keys("\u{1b}");
    rynx.wait_for(&site.url("/"));
    rynx.quit();
}

/// SET-14: the settings page saves what's changed on it, and refuses a bad home page
#[test]
fn set_14_settings_page() {
    let site = Site::new(&[("/new-home", "<title>New home</title><h1>The new home page</h1>")]);
    let (home, mut rynx) = started_with("settings-page", "");
    rynx.wait_for("RYNX BROWSER");
    rynx.keys("S");
    rynx.wait_for("Home page");
    // The home page field is the first text field after the search address: click below its label
    let (row, _) = rynx.find("Home page:").expect("the home page label");
    rynx.click(row + 1, 2);
    rynx.wait_for("Typing");
    rynx.type_text("javascript:x");
    rynx.keys("\u{1b}");
    rynx.keys("G");
    rynx.wait_for("Save settings");
    rynx.click_text("Save settings");
    rynx.wait_for("Not saved: invalid home_page");
    rynx.keys("g");
    let (row, _) = rynx.find("Home page:").unwrap();
    rynx.click(row + 1, 2);
    rynx.keys("\u{15}");
    rynx.type_text(&site.url("/new-home"));
    rynx.keys("\u{1b}");
    rynx.scroll_to("Save history to disk");
    rynx.click_text("Save history to disk");
    rynx.keys("G");
    rynx.click_text("Save settings");
    rynx.wait_for("Settings saved");
    let file = home.read_config("settings.conf").unwrap();
    assert!(file.contains(&format!("home_page = {}", site.url("/new-home"))), "{file}");
    assert!(file.contains("history = on"), "{file}");
    rynx.keys("n");
    rynx.wait_for("The new home page");
    rynx.quit();
}

/// SET-15: locked to I2P, rynx starts in I2P mode and p can't leave it; locked to the clearweb,
/// p can't enter it
#[test]
fn set_15_network_lock() {
    let (home, mut rynx) = started_with("i2p-lock", "network = i2p\n");
    rynx.wait_for("I2P mode");
    rynx.keys("p");
    rynx.wait_for("locked on in settings");
    rynx.quit();
    home.settings("network = clearweb\n");
    let mut again = Rynx::start(&home);
    again.wait_for("Clearweb");
    again.keys("p");
    again.wait_for("turned off in settings");
    again.quit();
}

/// KEY-01: F5 added to reload, Alt+l to the next tab, q taken from quit; about:keys lists them
#[test]
fn key_01_changed_keys() {
    let site = Site::new(&[("/", "<title>Counted</title><h1>A page</h1>")]);
    let home = Home::new("changed-keys");
    home.settings("");
    std::fs::write(home.config().join("keys.conf"), "reload = F5\nnext_tab = Alt+l\nquit = Ctrl+q only\n").unwrap();
    let mut rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    rynx.open(&site.url("/"));
    rynx.wait_for("A page");
    // F5 as xterm sends it
    rynx.keys("\x1b[15~");
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(site.requests("/"), 2, "F5 didn't reload");
    rynx.keys("n");
    rynx.wait_for("RYNX BROWSER");
    // Alt+l: Escape, then l
    rynx.keys("\x1bl");
    rynx.wait_for("A page");
    rynx.open("about:keys");
    rynx.scroll_to("r, F5");
    rynx.scroll_to("Alt+l");
    rynx.keys("q");
    std::thread::sleep(Duration::from_millis(400));
    assert!(rynx.is_running(), "q quit");
    rynx.keys("\u{11}");
    assert!(rynx.wait_exit());
}

/// UPD-02: a copy installed with Cargo is told the command, with no download button
#[test]
fn upd_02_cargo_installs() {
    let home = Home::new("cargo-install");
    home.settings("updates = daily\n");
    let program = home.0.join(".cargo/bin/rynx");
    std::fs::create_dir_all(program.parent().unwrap()).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_rynx"), &program).unwrap();
    let site = Site::new(&[]);
    let name = format!("rynx-99.0.0-{}", rynx::update::TARGET);
    let base = site.url("");
    let releases = serde_json::json!([{ "tag_name": "v99.0.0", "prerelease": false, "draft": false, "body": "New.",
        "html_url": format!("{base}/release"), "assets": [
            { "name": name, "browser_download_url": format!("{base}/program") },
            { "name": "SHA256SUMS", "browser_download_url": format!("{base}/sums") },
            { "name": "SHA256SUMS.sig", "browser_download_url": format!("{base}/sig") } ] }]);
    site.respond("/releases", wiremock::ResponseTemplate::new(200).set_body_json(releases));
    let env = [("RYNX_TEST_UPDATE_API", format!("{base}/releases"))];
    let mut rynx = Rynx::start_with(&program, &home, &[], &env);
    rynx.wait_for("rynx 99.0.0 is available");
    rynx.keys("U");
    rynx.wait_for("cargo install rynx --version 99.0.0");
    assert!(!rynx.text().contains("Download it"), "a download button for a Cargo install");
    rynx.quit();
}

/// UPD-03: locked to I2P, no update check is made, even when asked
#[test]
fn upd_03_no_checks_locked_to_i2p() {
    let site = Site::new(&[]);
    site.respond("/releases", wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!([])));
    let home = Home::new("update-i2p");
    home.settings("updates = daily\nnetwork = i2p\n");
    let env = [("RYNX_TEST_UPDATE_API", site.url("/releases"))];
    let mut rynx = Rynx::start_with(std::path::Path::new(env!("CARGO_BIN_EXE_rynx")), &home, &[], &env);
    rynx.wait_for("I2P mode");
    rynx.keys("U");
    rynx.wait_for("Check now");
    rynx.click_text("Check now");
    rynx.wait_for("locked to I2P");
    assert_eq!(site.requests("/releases"), 0, "an update check was made");
    rynx.quit();
}

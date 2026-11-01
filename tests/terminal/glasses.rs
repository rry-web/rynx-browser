//! Glasses mode (--glasses) and the control socket: rynx driven by naming actions, as the voice
//! controller in glasses/ does (GLASS-01 in TESTING.md). Unix only, like the socket.

use super::harness::*;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A short path for the socket: macOS allows socket paths of 104 bytes at most, and the test
/// homes are deep in the temporary folder
fn socket_path(name: &str) -> PathBuf {
    let dir = PathBuf::from(format!("/tmp/rynx-ctl-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("s.sock")
}

struct Control(BufReader<UnixStream>);

impl Control {
    fn connect(path: &Path) -> Control {
        for _ in 0..100 {
            if let Ok(stream) = UnixStream::connect(path) {
                stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
                return Control(BufReader::new(stream));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("the control socket never appeared at {}", path.display());
    }

    /// Send a command; its answer
    fn send(&mut self, command: &str) -> serde_json::Value {
        self.0.get_mut().write_all(format!("{command}\n").as_bytes()).unwrap();
        let mut answer = String::new();
        self.0.read_line(&mut answer).unwrap();
        serde_json::from_str(&answer).unwrap_or_else(|e| panic!("{e}: {answer:?}"))
    }
}

#[test]
fn glass_01_glasses_mode_driven_through_the_socket() {
    let site = Site::new(&[
        (
            "/a",
            "<title>Alpha</title><h1>Alpha page</h1><p>Some words about alpha. <a href=\"/b\">Go to beta</a> or <a href=\"/c\">gamma</a>.</p>",
        ),
        ("/b", "<title>Beta</title><h1>Beta page</h1><p>Beta's words.</p>"),
        ("/c", "<title>Gamma</title><h1>Gamma page</h1><p>Gamma's words.</p>"),
        (
            "/form",
            "<title>Form</title><h1>Search here</h1><form action=\"/results\"><input name=q></form><p>A form to search with, and some words around it for reader mode.</p>",
        ),
        ("/results", "<title>Results</title><h1>Results for potatoes</h1><p>The results of the search.</p>"),
    ]);
    let home = Home::new("glasses");
    home.settings("");
    let path = socket_path("glasses");
    let program = Path::new(env!("CARGO_BIN_EXE_rynx"));
    let mut rynx =
        Rynx::start_with(program, &home, &["--glasses"], &[("RYNX_CONTROL_SOCKET", path.display().to_string())]);
    rynx.wait_for("Glasses mode");
    let mut control = Control::connect(&path);

    let answer = control.send(&format!("open {}", site.url("/a")));
    assert_eq!(answer["ok"], true, "{answer}");
    rynx.wait_for("Alpha page");
    // No colour anywhere, and the page in reader mode
    assert_eq!(rynx.coloured_cells(), vec![], "{}", rynx.screen());
    assert!(rynx.screen().contains("READER"), "{}", rynx.screen());
    // The compact layout: the title on the first row, the page from the second, the status line
    // on the last; no tab bar, address bar or border
    let rows = rynx.rows();
    assert!(rows[0].contains("Alpha") && rows[0].contains("READER"), "{}", rynx.screen());
    assert!(rows[1].contains("Alpha page"), "{}", rynx.screen());
    assert!(!rynx.screen().contains("┌"), "{}", rynx.screen());
    rynx.keys("j");
    rynx.wait_for("Some words about alpha");
    let status = control.send("status");
    // The test's window is 140x40: rynx draws in the 80x30 screen glasses.conf (written now) has,
    // with its edge drawn, and the status line on the screen's last row
    assert!(home.read_config("glasses.conf").unwrap().contains("screen = 80x30"));
    let rows = rynx.rows();
    assert!(rows[29].contains(status["message"].as_str().unwrap()), "{}", rynx.screen());
    assert_eq!(rows[0].chars().nth(80), Some('│'), "{}", rynx.screen());
    assert!(rows[30].contains("glasses screen 80x30"), "{}", rynx.screen());
    assert_eq!(status["title"], "Alpha", "{status}");
    assert!(status["address"].as_str().unwrap().ends_with("/a"), "{status}");

    let read = control.send("read");
    assert!(read["text"].as_str().unwrap().contains("Some words about alpha"), "{read}");

    // The links as f numbers them: the second is gamma
    control.send("hints");
    rynx.wait_for("Follow which link");
    rynx.wait_for("1o to beta");
    assert_eq!(control.send("status")["mode"], "Hints");
    assert_eq!(control.send("follow 2")["ok"], true);
    rynx.wait_for("Gamma page");
    assert_eq!(control.send("follow 9")["ok"], false, "no link 9");

    control.send("back");
    rynx.wait_for("Alpha page");
    // A click lands on the link under it, with the page a row from the top
    rynx.click_text("Go to beta");
    rynx.wait_for("Beta page");
    control.send("back");
    rynx.wait_for("Alpha page");
    control.send("find beta");
    rynx.wait_for("1 match for \"beta\"");
    control.send("heard go back");
    rynx.wait_for("Heard: go back");

    // Typing while browsing is refused: the letters would be keys, and q quits
    let typed = control.send("type quick brown fox");
    assert_eq!(typed["ok"], false, "{typed}");
    assert!(rynx.is_running());
    // Into a field: chosen by its number, typed into, sent
    control.send(&format!("open {}", site.url("/form")));
    rynx.wait_for("Search here");
    control.send("follow 1");
    assert_eq!(control.send("status")["mode"], "FormEdit");
    assert_eq!(control.send("type potatoes")["ok"], true);
    control.send("key Enter");
    rynx.wait_for("Results for potatoes");
    control.send("back");
    control.send("back");
    rynx.wait_for("Alpha page");

    let unknown = control.send("fly away");
    assert_eq!(unknown["ok"], false, "{unknown}");
    assert!(unknown["message"].as_str().unwrap().contains("unknown command"), "{unknown}");
    assert!(control.send("actions")["actions"].as_array().unwrap().len() > 40);

    // A second rynx can't take the socket while this one has it
    let other = std::process::Command::new(program)
        .arg("--control-socket")
        .env("HOME", &home.0)
        .env("RYNX_CONTROL_SOCKET", &path)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&other.stderr).contains("another rynx is using"), "{other:?}");

    control.send("quit");
    assert!(rynx.wait_exit(), "{}", rynx.screen());
    // Gone with rynx (the file stays here: the sandbox lets rynx remove files only in its own
    // folders, as it does the default socket), and the next rynx takes its place
    assert!(UnixStream::connect(&path).is_err(), "the socket still answers");
    let mut again =
        Rynx::start_with(program, &home, &["--control-socket"], &[("RYNX_CONTROL_SOCKET", path.display().to_string())]);
    again.wait_for("Control socket on");
    Control::connect(&path).send("quit");
    assert!(again.wait_exit());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// A layout file for a small display (GLASS-04): the screen it says, the rows and margin it asks
#[test]
fn glass_04_a_layout_file_for_a_small_screen() {
    let words = "Many words about the small display, which wrap at its edge and not the window's. ".repeat(4);
    let site = Site::new(&[
        ("/a", &format!("<title>Small</title><h1>Small page</h1><p>{words}<a href=\"/b\">Next page</a></p>")),
        ("/b", "<title>Next</title><h1>The next page</h1><p>Words on the next page, for reader mode.</p>"),
    ]);
    let home = Home::new("glasses-small");
    home.settings("");
    let layout = home.config().join("small.conf");
    std::fs::write(&layout, "screen = 320x192 pixels\ntitle_row = off\nmargin = 0\n").unwrap();
    let path = socket_path("small");
    let program = Path::new(env!("CARGO_BIN_EXE_rynx"));
    let layout_arg = layout.display().to_string();
    let mut rynx = Rynx::start_with(
        program,
        &home,
        &["--glasses-layout", &layout_arg],
        &[("RYNX_CONTROL_SOCKET", path.display().to_string())],
    );
    rynx.wait_for("Glasses mode, 40x12");
    let mut control = Control::connect(&path);
    control.send(&format!("open {}", site.url("/a")));
    rynx.wait_for("Small page");
    let rows = rynx.rows();
    // No title row: the page from the first row, from the first column, wrapped at 40
    assert!(rows[0].starts_with("Small page"), "{}", rynx.screen());
    for row in &rows[..12] {
        let outside: String = row.chars().skip(40).collect();
        assert_eq!(outside.trim(), "│", "{}", rynx.screen());
    }
    assert!(rows[12].contains("glasses screen 40x12"), "{}", rynx.screen());
    assert!(rows[13..].iter().all(|r| r.trim().is_empty()), "{}", rynx.screen());
    // The status line on the screen's last row (cut to fit)
    let message: String = control.send("status")["message"].as_str().unwrap().chars().take(30).collect();
    assert!(rows[11].starts_with(&message), "{}", rynx.screen());
    // Clicks outside the screen do nothing; inside, they land on what's under them
    rynx.click(30, 100);
    rynx.settle();
    assert!(rynx.is_running());
    rynx.click_text("Next page");
    rynx.wait_for("The next page");
    control.send("quit");
    assert!(rynx.wait_exit());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// The menu (m) and push-to-talk (Space) (GLASS-05): chosen by number, by stepping, through the
/// socket; Page... and back; the talk key seen by the voice controller
#[test]
fn glass_05_the_menu_and_push_to_talk() {
    let site = Site::new(&[
        (
            "/a",
            "<title>Alpha</title><h1>Alpha page</h1><p>Some words about alpha, for reader mode. <a href=\"/b\">Go to beta</a> or <a href=\"/c\">gamma</a>.</p>",
        ),
        ("/b", "<title>Beta</title><h1>Beta page</h1><p>Beta's words, for reader mode to keep.</p>"),
        ("/c", "<title>Gamma</title><h1>Gamma page</h1><p>Gamma's words, for reader mode to keep.</p>"),
    ]);
    let home = Home::new("glasses-menu");
    home.settings("");
    let path = socket_path("menu");
    let program = Path::new(env!("CARGO_BIN_EXE_rynx"));
    let mut rynx =
        Rynx::start_with(program, &home, &["--glasses"], &[("RYNX_CONTROL_SOCKET", path.display().to_string())]);
    rynx.wait_for("Glasses mode");
    let mut control = Control::connect(&path);
    control.send(&format!("open {}", site.url("/a")));
    rynx.wait_for("Alpha page");

    // The first link is selected: the menu is about it
    rynx.keys("m");
    rynx.wait_for("1 Open the link");
    assert!(rynx.screen().contains("Link: http"), "{}", rynx.screen());
    assert_eq!(control.send("status")["mode"], "Menu");
    assert!(rynx.screen().contains("Talk (one voice command)"), "{}", rynx.screen());
    // Stepped to the second item (open in a new tab), and chosen
    rynx.keys("\t");
    rynx.keys("\r");
    rynx.wait_for("2/2 Beta");

    // Page... then back to the main menu, then out
    rynx.keys("m");
    rynx.wait_for("Page...");
    let page = rynx.rows().iter().position(|r| r.contains("Page...")).unwrap();
    let number = rynx.rows()[page].trim_start_matches(|c: char| !c.is_ascii_digit()).chars().next().unwrap();
    rynx.keys(&number.to_string());
    rynx.wait_for("Find on the page");
    rynx.keys("\x7f");
    rynx.wait_gone("Find on the page");
    rynx.wait_for("Tabs...");
    rynx.keys("\x1b");
    rynx.wait_gone("Tabs...");
    assert_eq!(control.send("status")["mode"], "Normal");

    // Through the socket, as voice does: the menu, its items by number, stepping with actions
    control.send("menu");
    assert_eq!(control.send("choose 99")["ok"], false);
    control.send("next_link");
    control.send("previous_link");
    let tabs = control.send("menu");
    assert_eq!(tabs["ok"], true, "{tabs}");
    assert_eq!(control.send("status")["mode"], "Normal", "menu again closes it");
    control.send("menu");
    rynx.wait_for("Tabs...");
    let tabs_item = rynx.rows().iter().find(|r| r.contains("Tabs...")).unwrap().trim().chars().nth(1).unwrap();
    control.send(&format!("choose {tabs_item}"));
    rynx.wait_for("Close the tab");
    control.send("choose 4");
    rynx.wait_for("Alpha page");
    assert_eq!(control.send("status")["tabs"], 1);
    assert_eq!(control.send("choose 1")["ok"], false, "no menu open");

    // Push-to-talk: Space turns listening on, which the voice controller sees, and off
    rynx.keys(" ");
    rynx.wait_for("Listening");
    assert_eq!(control.send("status")["listening"], true);
    assert!(rynx.rows()[0].contains("TALK"), "{}", rynx.screen());
    // The controller turns it off quietly once it has heard its command
    control.send("heard links");
    assert_eq!(control.send("talk off")["listening"], false);
    rynx.wait_gone("TALK");
    assert!(rynx.screen().contains("Heard: links"), "{}", rynx.screen());

    control.send("quit");
    assert!(rynx.wait_exit());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// Talking wherever you are, and typing by voice (GLASS-07): the talk key over numbered links
/// keeps them; in a field Space types and Ctrl+Space talks; begin fill, type, erase, send
#[test]
fn glass_07_talking_and_typing_by_voice() {
    let site = Site::new(&[
        (
            "/form",
            "<title>Form</title><h1>Search here</h1><p>Words before the form, for reader mode to keep.</p><form action=\"/results\"><input name=q value=\"old words\"></form>",
        ),
        ("/results", "<title>Results</title><h1>Results page</h1><p>The results of the search, for reader mode.</p>"),
    ]);
    let home = Home::new("glasses-talk");
    home.settings("");
    let path = socket_path("talk");
    let program = Path::new(env!("CARGO_BIN_EXE_rynx"));
    let mut rynx =
        Rynx::start_with(program, &home, &["--glasses"], &[("RYNX_CONTROL_SOCKET", path.display().to_string())]);
    rynx.wait_for("Glasses mode");
    let mut control = Control::connect(&path);
    control.send(&format!("open {}", site.url("/form")));
    rynx.wait_for("Search here");

    // Over numbered links, the talk key talks and the numbers stay
    rynx.keys("f");
    rynx.wait_for("Follow which link");
    rynx.keys(" ");
    rynx.wait_for("Listening");
    let status = control.send("status");
    assert_eq!((status["mode"].as_str(), status["listening"].as_bool()), (Some("Hints"), Some(true)), "{status}");
    rynx.keys(" ");
    rynx.wait_for("Stopped listening");
    rynx.keys("\x1b");

    // The page's field, emptied, typed into and partly taken back, by the voice controller
    let begun = control.send("begin fill");
    assert_eq!(begun["ok"], true, "{begun}");
    assert_eq!(control.send("status")["mode"], "FormEdit");
    rynx.wait_gone("old words");
    // In a field Space types; Ctrl+Space talks
    rynx.keys(" ");
    rynx.keys("\x00");
    rynx.wait_for("Listening");
    control.send("talk off");
    control.send("type potatoes and leeks");
    assert_eq!(control.send("erase 10")["ok"], true);
    rynx.wait_for("potatoes");
    rynx.wait_gone("and leeks");
    assert!(rynx.screen().contains("[ potatoes"), "{}", rynx.screen());
    control.send("key Enter");
    rynx.wait_for("Results page");
    let query = site.received("/results")[0].url.query().unwrap_or_default().to_string();
    assert_eq!(query, "q=+potatoes", "the space typed first, then the words, less the last ten");

    // Nowhere to type, or nothing being typed
    assert_eq!(control.send("begin fill")["ok"], false);
    assert_eq!(control.send("erase 2")["ok"], false);
    control.send("quit");
    assert!(rynx.wait_exit());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

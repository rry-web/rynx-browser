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
    let status = control.send("status");
    assert_eq!(status["title"], "Alpha", "{status}");
    assert!(status["address"].as_str().unwrap().ends_with("/a"), "{status}");

    let read = control.send("read");
    assert!(read["text"].as_str().unwrap().contains("Some words about alpha"), "{read}");

    // The links as f numbers them: the second is gamma
    control.send("hints");
    rynx.wait_for("Follow which link");
    assert_eq!(control.send("status")["mode"], "Hints");
    assert_eq!(control.send("follow 2")["ok"], true);
    rynx.wait_for("Gamma page");
    assert_eq!(control.send("follow 9")["ok"], false, "no link 9");

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

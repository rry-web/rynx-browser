//! Linux only: the sandbox's status, and no clipboard (PLAT-01, CLIP-02 in TESTING.md). CI's
//! Linux job runs these, along with every other terminal test inside the Landlock sandbox
//! (which covers PLAT-02, SET-06 and SET-10)
#![cfg(target_os = "linux")]

use super::harness::*;

/// PLAT-01: the status line says the sandbox is active (or why it isn't, on old kernels)
#[test]
fn plat_01_linux_sandbox_status() {
    let home = Home::new("linux-sandbox");
    home.settings("");
    let mut rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    rynx.wait_for("filesystem sandbox");
    let status = rynx.status();
    assert!(
        status.contains("filesystem sandbox active")
            || status.contains("partly active")
            || status.contains("kernel lacks Landlock"),
        "{status}"
    );
    rynx.quit();
}

/// CLIP-02: with no clipboard (no display, as here), rynx works; copying says so, and pasting
/// leaves the address alone
#[test]
fn clip_02_no_clipboard() {
    let site = Site::new(&[("/", "<p>Text to copy</p>")]);
    let home = Home::new("no-clipboard");
    home.settings("");
    let mut rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    rynx.open(&site.url("/"));
    rynx.wait_for("Text to copy");
    rynx.keys("v");
    rynx.keys("llll");
    rynx.keys("y");
    rynx.wait_for("Clipboard unavailable");
    rynx.keys("e");
    // Ctrl+k replaces the address with the clipboard; with none, the address stays
    rynx.keys("\u{b}");
    rynx.wait_for(&site.url("/"));
    rynx.keys("\u{1b}");
    rynx.quit();
}

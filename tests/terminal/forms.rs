//! Forms: fields, choices, passwords, where they send (FORM in TESTING.md)

use super::harness::*;
use std::time::{Duration, Instant};

fn started(name: &str) -> (Home, Rynx) {
    let home = Home::new(name);
    home.settings("");
    let rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    (home, rynx)
}

/// FORM-01: a search field says where it sends; typing and Enter search
#[test]
fn form_01_search_box() {
    let site = Site::new(&[(
        "/",
        "<p>Intro</p><form action=\"/w/index.php\"><input name=\"search\" placeholder=\"Search\"><button>Go</button></form>",
    )]);
    site.mount(
        wiremock::Mock::given(wiremock::matchers::path("/w/index.php"))
            .and(wiremock::matchers::query_param("search", "terminal browsers"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_raw("<h1>Results for terminal browsers</h1>", "text/html"),
            ),
    );
    let (_home, mut rynx) = started("search-box");
    rynx.open(&site.url("/"));
    rynx.wait_for("Intro");
    rynx.wait_for(&format!("Text field \"search\" → sends to {}", site.url("/w/index.php")));
    rynx.keys("\r");
    rynx.wait_for("Typing: Enter submits the form");
    rynx.type_text("terminal browsers");
    rynx.keys("\r");
    rynx.wait_for("Results for terminal browsers");
    rynx.quit();
}

/// FORM-03: checkboxes toggle, a radio choice clears the others, a choice cycles
#[test]
fn form_03_checkboxes_radios_and_choices() {
    let site = Site::new(&[(
        "/",
        "<form><input type=checkbox name=a> Exact<br><input type=radio name=r value=1 checked> One \
         <input type=radio name=r value=2> Two<br><select name=s><option>Red</option><option>Green</option></select></form>",
    )]);
    let (_home, mut rynx) = started("choices");
    rynx.open(&site.url("/"));
    rynx.wait_for("Exact");
    assert!(rynx.text().contains("[ ] Exact"), "{}", rynx.screen());
    // The checkbox is selected first: Enter ticks it
    rynx.keys("\r");
    rynx.wait_for("[x] Exact");
    // The second radio button: Enter picks it, clearing the first
    rynx.keys("\t\t\r");
    rynx.wait_for("( ) One (•) Two");
    // The choice cycles through its options
    rynx.keys("\t");
    rynx.wait_for("Red ▾");
    rynx.keys("\r");
    rynx.wait_for("Green ▾");
    rynx.keys("\r");
    rynx.wait_for("Red ▾");
    rynx.quit();
}

/// FORM-04: a password shows only as dots
#[test]
fn form_04_passwords_show_as_dots() {
    let site = Site::new(&[("/", "<form><input type=password name=pw></form>")]);
    let (_home, mut rynx) = started("password");
    rynx.open(&site.url("/"));
    rynx.wait_for("[____");
    rynx.keys("\r");
    rynx.type_text("hunter2");
    rynx.wait_for("•••••••");
    assert!(!rynx.text().contains("hunter2"));
    rynx.keys("\u{1b}");
    assert!(!rynx.text().contains("hunter2"));
    rynx.quit();
}

/// FORM-05: typing into a field on a large page keeps up
#[test]
fn form_05_typing_keeps_up_on_a_large_page() {
    let mut html = String::from("<form><input name=q size=60></form>");
    for i in 0..3000 {
        html.push_str(&format!("<p>Line {i} with <a href=\"/l{i}\">a link</a> and more words around it.</p>"));
    }
    let site = Site::new(&[("/", &html)]);
    let (_home, mut rynx) = started("typing");
    rynx.open(&site.url("/"));
    rynx.wait_for_within("Line 0 with", Duration::from_secs(20));
    rynx.keys("\r");
    let start = Instant::now();
    rynx.type_text("the quick brown fox jumps");
    rynx.wait_for_within("the quick brown fox jumps", Duration::from_secs(2));
    assert!(start.elapsed() < Duration::from_secs(3), "typing lagged: {:?}", start.elapsed());
    rynx.keys("\u{1b}");
    rynx.quit();
}

/// FORM-06: a local page's form may send to a local address
#[test]
fn form_06_local_pages_may_send_locally() {
    let site = Site::new(&[("/", "<form action=\"/save\"><input name=name value=router><button>Save</button></form>")]);
    site.page("/save", "<h1>Saved the router settings</h1>");
    let (_home, mut rynx) = started("local-form");
    rynx.open(&site.url("/"));
    rynx.wait_for("[Save]");
    rynx.click_text("[Save]");
    rynx.wait_for("Saved the router settings");
    rynx.quit();
}

/// FORM-07: a link disguising its site with a user name is flagged, and not followed
#[test]
fn form_07_disguised_links() {
    let site = Site::new(&[("/", "<p><a href=\"https://paypal.com@example.com/login\">PayPal login</a></p>")]);
    let (_home, mut rynx) = started("disguised");
    rynx.open(&site.url("/"));
    rynx.wait_for("PayPal login");
    rynx.wait_for("⚠ really goes to example.com");
    rynx.keys("\r");
    rynx.wait_for("example.com");
    assert!(rynx.status().contains("example.com"), "{}", rynx.status());
    assert!(rynx.text().contains("PayPal login"), "it was followed");
    rynx.quit();
}

/// FORM-08: a password typed in an address is hidden, and never bookmarked
#[test]
fn form_08_passwords_in_typed_addresses() {
    let site = Site::new(&[("/", "<title>Private</title><h1>Private area</h1>")]);
    let (home, mut rynx) = started("address-password");
    let address = site.url("/").replace("http://", "http://user:secret@");
    rynx.open(&address);
    rynx.wait_for("Private area");
    // Once loaded, the address bar shows the page's address, without the name and password
    rynx.wait_for(&site.url("/"));
    assert!(!rynx.text().contains("secret"));
    rynx.keys("b");
    rynx.settle();
    let bookmarks = home.read_config("bookmarks.txt").unwrap_or_default();
    assert!(bookmarks.contains("127.0.0.1"), "not bookmarked: {bookmarks:?}");
    assert!(!bookmarks.contains("secret") && !bookmarks.contains("user:"), "{bookmarks}");
    rynx.quit();
}

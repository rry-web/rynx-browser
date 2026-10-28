use reqwest::cookie::CookieStore;
use reqwest::header::HeaderValue;
use rynx::cookies::{Jar, site_of};
use url::Url;

fn url(text: &str) -> Url {
    Url::parse(text).unwrap()
}

fn set(jar: &Jar, page: &str, headers: &[&str]) {
    let values: Vec<HeaderValue> = headers.iter().map(|h| HeaderValue::from_str(h).unwrap()).collect();
    jar.set_cookies(&mut values.iter(), &url(page));
}

fn sent(jar: &Jar, page: &str) -> String {
    jar.cookies(&url(page)).map(|v| v.to_str().unwrap().to_string()).unwrap_or_default()
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rynx-cookies-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_site_is_what_is_registered_under_a_public_suffix() {
    assert_eq!(site_of("accounts.example.com"), "example.com");
    assert_eq!(site_of("www.example.com"), "example.com");
    assert_eq!(site_of("WWW.BBC.CO.UK."), "bbc.co.uk");
    assert_eq!(site_of("alice.github.io"), "alice.github.io");
    assert_ne!(site_of("alice.github.io"), site_of("bob.github.io"));
    assert_eq!(site_of("192.168.1.1"), "192.168.1.1");
    assert_eq!(site_of("forum.stats.i2p"), "forum.stats.i2p");
}

#[test]
fn no_cookies_unless_the_site_is_allowed() {
    let jar = Jar::default();
    set(&jar, "https://www.example.com/login", &["session=abc; Path=/; Max-Age=3600"]);
    assert_eq!(sent(&jar, "https://www.example.com/account"), "");

    jar.allow("www.example.com").unwrap();
    assert!(jar.allowed("accounts.example.com"), "the whole site is allowed");
    // Nothing was kept from before it was allowed
    assert_eq!(sent(&jar, "https://www.example.com/account"), "");
    set(&jar, "https://accounts.example.com/login", &["session=abc; Domain=example.com; Path=/; Secure; Max-Age=3600"]);
    assert_eq!(sent(&jar, "https://www.example.com/account"), "session=abc");
    // Secure cookies only over https, and never to another site
    assert_eq!(sent(&jar, "http://www.example.com/account"), "");
    assert_eq!(sent(&jar, "https://example.org/"), "");
    assert_eq!(jar.sites(), vec![("example.com".to_string(), 1)]);
}

#[test]
fn a_cookie_for_a_whole_public_suffix_is_refused() {
    let jar = Jar::default();
    jar.allow("shop.example.co.uk").unwrap();
    jar.allow("alice.github.io").unwrap();
    set(
        &jar,
        "https://shop.example.co.uk/",
        &["wide=1; Domain=co.uk; Max-Age=60", "own=1; Domain=example.co.uk; Max-Age=60"],
    );
    set(&jar, "https://alice.github.io/", &["wide=1; Domain=github.io; Max-Age=60"]);
    jar.allow("bob.github.io").unwrap();
    assert_eq!(sent(&jar, "https://shop.example.co.uk/"), "own=1");
    assert_eq!(sent(&jar, "https://bob.github.io/"), "", "alice's cookie reached bob");
}

#[test]
fn forgetting_a_site_signs_you_out() {
    let jar = Jar::default();
    jar.allow("example.com").unwrap();
    jar.allow("example.org").unwrap();
    set(&jar, "https://example.com/", &["a=1; Max-Age=60", "b=2"]);
    set(&jar, "https://example.org/", &["c=3; Max-Age=60"]);
    jar.forget("www.example.com").unwrap();
    assert!(!jar.allowed("example.com"));
    assert_eq!(sent(&jar, "https://example.com/"), "");
    // Allowed again, the old cookies don't come back
    jar.allow("example.com").unwrap();
    assert_eq!(sent(&jar, "https://example.com/"), "");
    assert_eq!(sent(&jar, "https://example.org/"), "c=3");
}

#[test]
fn anubis_passes_are_kept_for_every_site() {
    let jar = Jar::default();
    let pass = "https://www.site.example/.within.website/x/cmd/anubis/api/pass-challenge";
    set(&jar, pass, &["techaro.lol-anubis-auth=jwt; Path=/"]);
    // An ordinary page's cookie isn't
    set(&jar, "https://www.site.example/fiction", &["tracker=1; Path=/"]);
    assert_eq!(sent(&jar, "https://www.site.example/fiction"), "techaro.lol-anubis-auth=jwt");
}

#[test]
fn lasting_cookies_are_saved_and_session_ones_end_at_quit() {
    let dir = temp_dir("saved");
    let jar = Jar::default();
    jar.load(&dir, Err("no keychain in tests".to_string()));
    jar.allow("example.com").unwrap();
    set(&jar, "https://example.com/login", &["remember=yes; Max-Age=86400; Path=/", "session=now; Path=/"]);
    let both = sent(&jar, "https://example.com/");
    assert!(both.contains("remember=yes") && both.contains("session=now"), "{both}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for file in [rynx::cookies::COOKIES_FILE, rynx::cookies::SITES_FILE] {
            let mode = std::fs::metadata(dir.join(file)).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "{file} is readable by others");
        }
    }

    // rynx starts again
    let again = Jar::default();
    again.load(&dir, Err("no keychain in tests".to_string()));
    assert!(again.allowed("www.example.com"));
    assert_eq!(sent(&again, "https://example.com/"), "remember=yes");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn saved_cookies_of_a_site_no_longer_allowed_are_dropped() {
    let dir = temp_dir("stale");
    let jar = Jar::default();
    jar.load(&dir, Err("no keychain in tests".to_string()));
    jar.allow("example.com").unwrap();
    jar.allow("example.org").unwrap();
    set(&jar, "https://example.com/", &["a=1; Max-Age=600"]);
    set(&jar, "https://example.org/", &["b=2; Max-Age=600"]);
    // The sites file edited by hand, leaving example.org out
    std::fs::write(dir.join(rynx::cookies::SITES_FILE), "example.com\n").unwrap();
    let again = Jar::default();
    again.load(&dir, Err("no keychain in tests".to_string()));
    again.allow("example.org").unwrap();
    assert_eq!(sent(&again, "https://example.com/"), "a=1");
    assert_eq!(sent(&again, "https://example.org/"), "", "a cookie outlived its site being removed");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn i2p_cookies_are_never_saved() {
    let jar = Jar::default();
    assert!(!jar.is_saved());
    jar.allow("forum.i2p").unwrap();
    set(&jar, "http://forum.i2p/", &["s=1; Max-Age=600"]);
    assert_eq!(sent(&jar, "http://forum.i2p/"), "s=1");
}

/// Signing in to a real (local) server: the cookie set at sign-in is sent with the next page
/// only when the site is allowed
#[tokio::test]
async fn signing_in_and_staying_signed_in() {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/login"))
        .respond_with(
            ResponseTemplate::new(303)
                .insert_header("location", "/account")
                .append_header("set-cookie", "session=s3cret; Path=/; HttpOnly"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/account"))
        .and(header("cookie", "session=s3cret"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Welcome back"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/account"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Please sign in"))
        .mount(&server)
        .await;

    let manager = rynx::network::NetworkManager::new().unwrap();
    let client = manager.get_client(false);
    let login = format!("{}/login", server.uri());
    let sign_in =
        || rynx::network::send_following_redirects(client, &login, false, Some("user=a&password=b".to_string()));

    let page = sign_in().await.unwrap().text().await.unwrap();
    assert_eq!(page, "Please sign in", "a cookie was kept for a site that wasn't allowed");

    manager.cookies(false).allow("127.0.0.1").unwrap();
    let page = sign_in().await.unwrap().text().await.unwrap();
    assert_eq!(page, "Welcome back");
    // The I2P clients have their own cookies
    assert!(!manager.cookies(true).allowed("127.0.0.1"));
}

#[tokio::test]
async fn c_lets_the_site_keep_cookies_and_c_again_forgets_them() {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = rynx::app::App::new(tx, rx).unwrap();
    app.render_in_background = false;
    app.current_tab().page_url = Some("https://forum.example.com/login".to_string());
    app.toggle_cookies();
    assert!(app.current_tab().status_message.starts_with("example.com may keep cookies until rynx quits"));
    assert!(app.network_manager.cookies(false).allowed("www.example.com"));

    // Changing the browser identity in settings rebuilds the network clients; the cookies
    // carry over to the new ones
    let before = std::sync::Arc::as_ptr(&app.network_manager);
    app.follow_link(rynx::app::SETTINGS_PAGE.to_string());
    let tab = app.current_tab();
    let field = tab.controls.iter().position(|c| c.name == "user_agent").expect("the user agent field");
    tab.control_values[field] = rynx::forms::ControlValue::Text("Custom/1.0".to_string());
    let form = tab.controls[field].form.unwrap();
    app.submit_form(form, None);
    let status = app.current_tab().status_message.clone();
    assert_eq!(app.settings.user_agent, "Custom/1.0", "{status}");
    assert_ne!(std::sync::Arc::as_ptr(&app.network_manager), before, "the clients weren't rebuilt");
    assert!(app.network_manager.cookies(false).allowed("example.com"), "the cookies were lost");

    app.current_tab().page_url = Some("https://forum.example.com/login".to_string());
    app.toggle_cookies();
    assert!(app.current_tab().status_message.contains("forgotten"));
    assert!(!app.network_manager.cookies(false).allowed("example.com"));

    app.current_tab().page_url = Some("gemini://capsule.example/".to_string());
    app.toggle_cookies();
    assert_eq!(app.current_tab().status_message, "Only web sites keep cookies");
}

#[tokio::test]
async fn the_cookies_page_lists_sites_and_forgets_them() {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = rynx::app::App::new(tx, rx).unwrap();
    app.render_in_background = false;
    app.network_manager.cookies(false).allow("example.com").unwrap();
    app.network_manager.cookies(false).allow("example.org").unwrap();
    app.follow_link(rynx::cookies::COOKIES_PAGE.to_string());
    let tab = app.current_tab();
    let text: String = tab.rendered_content.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n");
    assert!(text.contains("example.com") && text.contains("example.org"), "{text}");
    let forget =
        tab.controls.iter().position(|c| c.name == "forget" && c.value == "example.org").expect("a Forget button");
    let form = tab.controls[forget].form.unwrap();
    app.submit_form(form, Some(forget));
    assert!(!app.network_manager.cookies(false).allowed("example.org"));
    assert!(app.network_manager.cookies(false).allowed("example.com"));
    assert_eq!(app.current_tab().status_message, "example.org's cookies are forgotten");
}

/// A sign-in page from a site that can't keep cookies says how to stay signed in
#[tokio::test]
async fn a_sign_in_page_says_how_to_stay_signed_in() {
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    let form =
        "<form method=post action=/login><input name=user><input type=password name=pw><button>Sign in</button></form>";
    Mock::given(path("/login"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(form, "text/html"))
        .mount(&server)
        .await;
    Mock::given(path("/about"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("<p>About</p>", "text/html"))
        .mount(&server)
        .await;
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = rynx::app::App::new(tx, rx).unwrap();
    app.render_in_background = false;
    async fn open(app: &mut rynx::app::App, url: String) {
        app.follow_link(url);
        while let Ok(Some(response)) = tokio::time::timeout(std::time::Duration::from_secs(5), app.rx.recv()).await {
            let done = matches!(
                response,
                rynx::network::NetworkResponse::Success(..) | rynx::network::NetworkResponse::Error(..)
            );
            rynx::event_handler::handle_network_event(app, response, 80).unwrap();
            if done {
                return;
            }
        }
    }
    open(&mut app, format!("{}/login", server.uri())).await;
    assert_eq!(
        app.current_tab().status_message,
        "To stay signed in to 127.0.0.1, press C before signing in (it lets the site keep cookies)"
    );
    open(&mut app, format!("{}/about", server.uri())).await;
    assert!(!app.current_tab().status_message.contains("press C"), "no password asked here");
    // Once allowed, no hint
    app.network_manager.cookies(false).allow("127.0.0.1").unwrap();
    open(&mut app, format!("{}/login", server.uri())).await;
    assert!(!app.current_tab().status_message.contains("press C"));
}

fn files(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> =
        std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

#[test]
fn saved_cookies_are_encrypted_with_the_keychain_key() {
    let dir = temp_dir("encrypted");
    let key = [7u8; 32];
    let jar = Jar::default();
    jar.load(&dir, Ok(key));
    jar.allow("example.com").unwrap();
    set(&jar, "https://example.com/login", &["remember=t0ken-value; Max-Age=86400; Path=/"]);
    assert_eq!(files(&dir), vec![rynx::cookies::ENCRYPTED_FILE.to_string()]);
    let data = std::fs::read(dir.join(rynx::cookies::ENCRYPTED_FILE)).unwrap();
    let text = String::from_utf8_lossy(&data);
    assert!(!text.contains("t0ken-value") && !text.contains("example.com"), "readable on disk: {text}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(dir.join(rynx::cookies::ENCRYPTED_FILE)).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    // Started again with the same key: signed in still
    let again = Jar::default();
    again.load(&dir, Ok(key));
    assert_eq!(sent(&again, "https://example.com/"), "remember=t0ken-value");
    assert!(again.storage_note().starts_with("encrypted"), "{}", again.storage_note());

    // A different key (the keychain's entry replaced): nothing is read, and the file is kept
    let other = Jar::default();
    other.load(&dir, Ok([8u8; 32]));
    assert_eq!(sent(&other, "https://example.com/"), "");
    assert!(other.storage_note().contains("couldn't be read"), "{}", other.storage_note());
    assert_eq!(std::fs::read(dir.join(rynx::cookies::ENCRYPTED_FILE)).unwrap(), data);

    // No keychain now: started without them, the file kept
    let none = Jar::default();
    none.load(&dir, Err("no keychain".to_string()));
    assert!(!none.allowed("example.com"));
    assert!(none.storage_note().contains("isn't available now"), "{}", none.storage_note());
    assert_eq!(std::fs::read(dir.join(rynx::cookies::ENCRYPTED_FILE)).unwrap(), data);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_changed_cookies_file_is_refused() {
    let dir = temp_dir("tampered");
    let key = [9u8; 32];
    let jar = Jar::default();
    jar.load(&dir, Ok(key));
    jar.allow("example.com").unwrap();
    set(&jar, "https://example.com/", &["a=1; Max-Age=600"]);
    let path = dir.join(rynx::cookies::ENCRYPTED_FILE);
    let mut data = std::fs::read(&path).unwrap();
    let last = data.len() - 1;
    data[last] ^= 1;
    std::fs::write(&path, &data).unwrap();
    let again = Jar::default();
    again.load(&dir, Ok(key));
    assert!(!again.allowed("example.com"));
    assert!(again.storage_note().contains("changed"), "{}", again.storage_note());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn plain_cookies_from_before_are_encrypted_once_there_is_a_key() {
    let dir = temp_dir("migrate");
    let jar = Jar::default();
    jar.load(&dir, Err("no keychain yet".to_string()));
    assert!(jar.storage_note().starts_with("not encrypted"), "{}", jar.storage_note());
    jar.allow("example.com").unwrap();
    set(&jar, "https://example.com/", &["a=1; Max-Age=600"]);
    assert_eq!(files(&dir), vec![rynx::cookies::SITES_FILE.to_string(), rynx::cookies::COOKIES_FILE.to_string()]);

    let key = [3u8; 32];
    let again = Jar::default();
    again.load(&dir, Ok(key));
    assert_eq!(files(&dir), vec![rynx::cookies::ENCRYPTED_FILE.to_string()], "the plain files were left behind");
    assert_eq!(sent(&again, "https://example.com/"), "a=1");
    let third = Jar::default();
    third.load(&dir, Ok(key));
    assert_eq!(sent(&third, "https://example.com/"), "a=1");
    let _ = std::fs::remove_dir_all(&dir);
}

/// With the real system keychain: a key is made once and given back after. Uses its own
/// keychain entry, removed afterwards. `cargo test --test cookies -- --ignored`
#[test]
#[ignore]
fn the_system_keychain_keeps_the_key() {
    let service = "rynx-browser-test";
    let first = rynx::cookies::keychain_key(service).expect("a keychain");
    let second = rynx::cookies::keychain_key(service).unwrap();
    assert_eq!(first, second);
    assert_ne!(first, [0u8; 32]);
    let _ = keyring::Entry::new(service, "cookie encryption key").and_then(|e| e.delete_credential());
}

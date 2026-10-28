//! Updates: finding a release, verifying its signature and checksum, installing at the next start.

use ring::signature::{Ed25519KeyPair, KeyPair};
use rynx::app::App;
use rynx::event_handler::handle_network_event;
use rynx::network::{NetworkResponse, PageKind};
use rynx::update::{self, Installed, checksum_for, newest, program_name, verify};
use serde_json::json;
use std::path::PathBuf;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn key_pair() -> Ed25519KeyPair {
    let document = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
    Ed25519KeyPair::from_pkcs8(document.as_ref()).unwrap()
}

fn public(pair: &Ed25519KeyPair) -> [u8; 32] {
    pair.public_key().as_ref().try_into().unwrap()
}

fn sha256(data: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, data).as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

fn version(text: &str) -> semver::Version {
    semver::Version::parse(text).unwrap()
}

/// GitHub's release list, with the assets a release needs for this platform
fn release(tag: &str, prerelease: bool, base: &str) -> serde_json::Value {
    let name = program_name(&version(tag.trim_start_matches('v')));
    json!({
        "tag_name": tag, "prerelease": prerelease, "draft": false, "body": format!("Notes for {tag}\n\n<script>x</script>"),
        "html_url": format!("{base}/releases/{tag}"),
        "assets": [
            { "name": name, "browser_download_url": format!("{base}/{tag}/program") },
            { "name": "SHA256SUMS", "browser_download_url": format!("{base}/{tag}/SHA256SUMS") },
            { "name": "SHA256SUMS.sig", "browser_download_url": format!("{base}/{tag}/SHA256SUMS.sig") },
        ]
    })
}

#[test]
fn the_newest_suitable_release_is_chosen() {
    let base = "https://github.example";
    let mut no_build = release("v9.9.9", false, base);
    no_build["assets"].as_array_mut().unwrap().remove(0);
    let mut draft = release("v8.0.0", false, base);
    draft["draft"] = json!(true);
    let list = json!([
        release("v0.1.0", false, base),
        release("v0.3.0-beta.2", true, base),
        release("v0.2.5", false, base),
        no_build,
        draft
    ])
    .to_string();
    let current = version("0.2.0-beta.5");
    assert_eq!(newest(&list, &current, true).unwrap().version, version("0.3.0-beta.2"));
    assert_eq!(newest(&list, &current, false).unwrap().version, version("0.2.5"), "stable channel skips betas");
    assert!(newest(&list, &version("0.4.0"), true).is_none(), "nothing newer");
}

#[test]
fn checksums_and_signatures_are_checked() {
    let list = "aa  other\n0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  rynx-1.0.0-x\n";
    assert_eq!(
        checksum_for(list, "rynx-1.0.0-x").as_deref(),
        Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
    );
    assert_eq!(checksum_for(list, "other"), None, "a malformed line doesn't count");

    let pair = key_pair();
    let signature = pair.sign(list.as_bytes());
    assert!(verify(list.as_bytes(), signature.as_ref(), &public(&pair)));
    assert!(!verify(b"changed list", signature.as_ref(), &public(&pair)));
    assert!(!verify(list.as_bytes(), signature.as_ref(), &public(&key_pair())), "another key's signature");
    // The key built into rynx is the release key, not a test one
    assert_ne!(update::RELEASE_KEY, public(&pair));
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rynx-update-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A fake GitHub: the release list and a release's files, its checksums signed by `signer`
async fn github(signer: &Ed25519KeyPair, program: &[u8], listed_checksum: Option<String>) -> MockServer {
    let server = MockServer::start().await;
    let tag = "v99.0.0-beta.1";
    let name = program_name(&version("99.0.0-beta.1"));
    let sums = format!("{}  {}\n", listed_checksum.unwrap_or_else(|| sha256(program)), name);
    let signature = signer.sign(sums.as_bytes()).as_ref().to_vec();
    Mock::given(path("/releases"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(json!([release(tag, true, &server.uri())]).to_string(), "application/json"),
        )
        .mount(&server)
        .await;
    Mock::given(path(format!("/{tag}/program")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(program.to_vec()))
        .mount(&server)
        .await;
    Mock::given(path(format!("/{tag}/SHA256SUMS")))
        .respond_with(ResponseTemplate::new(200).set_body_string(sums))
        .mount(&server)
        .await;
    Mock::given(path(format!("/{tag}/SHA256SUMS.sig")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(signature))
        .mount(&server)
        .await;
    server
}

fn test_app(server: &MockServer, key: [u8; 32], dir: &std::path::Path) -> App {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(tx, rx).unwrap();
    app.clipboard = None;
    app.render_in_background = false;
    app.update_api = format!("{}/releases", server.uri());
    app.release_keys = vec![key];
    app.update_dir = Some(dir.join("update"));
    app.installed = Installed::Updatable(dir.join("rynx"));
    app
}

fn press_button(app: &mut App, name: &str) {
    let tab = app.current_tab();
    let button = tab.controls.iter().position(|c| c.name == name).unwrap_or_else(|| panic!("no {name} button"));
    let form = tab.controls[button].form.unwrap();
    app.submit_form(form, Some(button));
}

async fn next_update_message(app: &mut App) {
    loop {
        let resp = tokio::time::timeout(std::time::Duration::from_secs(10), app.rx.recv()).await.unwrap().unwrap();
        let done = matches!(resp, NetworkResponse::UpdateChecked(..) | NetworkResponse::UpdateDownloaded(..));
        handle_network_event(app, resp, 80).unwrap();
        if done {
            return;
        }
    }
}

fn text(app: &mut App) -> String {
    app.current_tab().rendered_content.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")
}

#[tokio::test]
async fn an_update_is_found_downloaded_verified_and_installed() {
    let pair = key_pair();
    let program = b"#!/bin/sh\necho the new rynx\n".to_vec();
    let server = github(&pair, &program, None).await;
    let dir = temp_dir("works");
    let mut app = test_app(&server, public(&pair), &dir);

    app.follow_link(update::UPDATE_PAGE.into());
    press_button(&mut app, "check");
    next_update_message(&mut app).await;
    assert!(
        app.current_tab().status_message.contains("rynx 99.0.0-beta.1 is available"),
        "{}",
        app.current_tab().status_message
    );
    let page = text(&mut app);
    assert!(
        page.contains("Notes for v99.0.0-beta.1") && page.contains("<script>x</script>"),
        "notes are shown as text: {page}"
    );

    press_button(&mut app, "install");
    next_update_message(&mut app).await;
    assert!(
        app.current_tab().status_message.contains("installed when rynx next starts"),
        "{}",
        app.current_tab().status_message
    );
    assert!(text(&mut app).contains("is downloaded and checked"));

    // The next start installs it over the program
    let program_file = dir.join("rynx");
    std::fs::write(&program_file, b"old").unwrap();
    let installed = update::install_pending(&dir.join("update"), Some(&program_file)).unwrap();
    assert_eq!(installed.as_deref(), Some("99.0.0-beta.1"));
    assert_eq!(std::fs::read(&program_file).unwrap(), program);
    assert!(update::pending(&dir.join("update")).is_none(), "nothing left waiting");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_release_signed_with_another_key_is_refused() {
    let impostor = key_pair();
    let server = github(&impostor, b"evil program", None).await;
    let dir = temp_dir("impostor");
    let mut app = test_app(&server, public(&key_pair()), &dir);
    app.follow_link(update::UPDATE_PAGE.into());
    press_button(&mut app, "check");
    next_update_message(&mut app).await;
    press_button(&mut app, "install");
    next_update_message(&mut app).await;
    assert!(app.current_tab().status_message.contains("signature isn't valid"), "{}", app.current_tab().status_message);
    assert!(update::pending(&dir.join("update")).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_download_that_doesnt_match_its_signed_checksum_is_refused() {
    let pair = key_pair();
    // Correctly signed, but for different contents than what's served
    let server = github(&pair, b"served program", Some(sha256(b"the program that was signed"))).await;
    let dir = temp_dir("mismatch");
    let mut app = test_app(&server, public(&pair), &dir);
    app.follow_link(update::UPDATE_PAGE.into());
    press_button(&mut app, "check");
    next_update_message(&mut app).await;
    press_button(&mut app, "install");
    next_update_message(&mut app).await;
    assert!(app.current_tab().status_message.contains("doesn't match"), "{}", app.current_tab().status_message);
    assert!(update::pending(&dir.join("update")).is_none());

    // A waiting update changed after it was checked isn't installed either
    std::fs::create_dir_all(dir.join("update")).unwrap();
    std::fs::write(dir.join("update/rynx-new"), b"tampered").unwrap();
    std::fs::write(
        dir.join("update/pending.json"),
        json!({"version": "99.0.0", "file": "rynx-new", "sha256": sha256(b"original")}).to_string(),
    )
    .unwrap();
    let program_file = dir.join("rynx");
    std::fs::write(&program_file, b"old").unwrap();
    assert!(update::install_pending(&dir.join("update"), Some(&program_file)).is_err());
    assert_eq!(std::fs::read(&program_file).unwrap(), b"old");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn cargo_installs_are_told_the_command_and_i2p_never_checks() {
    let pair = key_pair();
    let server = github(&pair, b"x", None).await;
    let dir = temp_dir("cargo");
    let mut app = test_app(&server, public(&pair), &dir);
    app.installed = Installed::Cargo;
    app.follow_link(update::UPDATE_PAGE.into());
    press_button(&mut app, "check");
    next_update_message(&mut app).await;
    let page = text(&mut app);
    assert!(page.contains("cargo install rynx --version 99.0.0-beta.1"), "{page}");
    assert!(!app.current_tab().controls.iter().any(|c| c.name == "install"));

    let (locked, _) = rynx::settings::Settings::parse("network = i2p");
    app.settings = locked;
    app.i2p_mode = true;
    press_button(&mut app, "check");
    assert!(app.current_tab().status_message.contains("locked to I2P"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn web_pages_cannot_start_an_update() {
    let pair = key_pair();
    let server = github(&pair, b"x", None).await;
    let dir = temp_dir("web");
    let mut app = test_app(&server, public(&pair), &dir);
    let id = app.current_tab().id;
    let html = r#"<form action="about:update-do"><input type=submit name=install value=Go></form>"#;
    app.current_tab().url_input = "https://evil.example/".into();
    let resp = NetworkResponse::Success(id, "t".into(), html.into(), "https://evil.example/".into(), PageKind::Html);
    handle_network_event(&mut app, resp, 80).unwrap();
    app.submit_form(0, Some(0));
    assert!(app.current_tab().status_message.starts_with("Not sent"), "{}", app.current_tab().status_message);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
#[ignore = "needs the internet"]
async fn live_github_release_list_is_read() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let manager = rynx::network::NetworkManager::new().unwrap();
    let found = update::check(manager.get_client(false), update::RELEASES_API, true).await.expect("the list is read");
    // Releases before signing began have no SHA256SUMS.sig, so they're never offered
    println!("offered: {:?}", found.map(|r| r.version));
}

/// The keys built into rynx are the real ones: the release key verifies a real release's
/// signed checksums, and the backup key a message signed with its (offline) private half
#[test]
fn the_built_in_keys_are_the_real_ones() {
    let sums = include_bytes!("data/release-0.2.0-beta.7-SHA256SUMS");
    let sums_signature = include_bytes!("data/release-0.2.0-beta.7-SHA256SUMS.sig");
    assert!(verify(sums, sums_signature, &update::RELEASE_KEY));
    assert!(!verify(sums, sums_signature, &update::BACKUP_KEY));
    let message = include_bytes!("data/backup-key-message.txt");
    let message_signature = include_bytes!("data/backup-key-message.sig");
    assert!(verify(message, message_signature, &update::BACKUP_KEY));
    assert!(!verify(message, message_signature, &update::RELEASE_KEY));
    assert_eq!(update::RELEASE_KEYS, [update::RELEASE_KEY, update::BACKUP_KEY]);
}

/// A release signed with the backup key (after the release key was lost) is installed
#[tokio::test]
async fn a_release_signed_with_the_backup_key_is_accepted() {
    let (release_key, backup_key) = (key_pair(), key_pair());
    let program = b"#!/bin/sh\necho signed with the backup key\n".to_vec();
    let server = github(&backup_key, &program, None).await;
    let dir = temp_dir("backup");
    let manager = rynx::network::NetworkManager::new().unwrap();
    let client = manager.get_client(false);
    let release = update::check(client, &format!("{}/releases", server.uri()), true).await.unwrap().expect("a release");
    let keys = [public(&release_key), public(&backup_key)];
    let pending = update::download(client, &release, &dir.join("update"), &keys).await.expect("accepted");
    assert_eq!(std::fs::read(&pending.file).unwrap(), program);
    // Signed by neither, it's refused
    let others = [public(&key_pair()), public(&release_key)];
    assert!(update::download(client, &release, &dir.join("update2"), &others).await.is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

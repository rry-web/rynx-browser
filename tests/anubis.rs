//! Anubis bot checks, passed without JavaScript, against a fake Anubis that checks answers the
//! way the real one does.

use rynx::anubis::{self, Challenge};
use rynx::app::App;
use rynx::event_handler::handle_network_event;
use rynx::network::NetworkResponse;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const API: &str = "/.within.website/x/cmd/anubis/api/pass-challenge";
const CSS: &str = "/.within.website/x/cmd/anubis/ext/css-load/";

struct Issued {
    random: String,
    at: Instant,
}

#[derive(Default)]
struct State {
    issued: HashMap<String, Issued>,
    css_fetched: Vec<String>,
    paths: Vec<String>,
    passes: usize,
}

/// A fake Anubis guarding every page with one challenge method
struct FakeAnubis {
    algorithm: &'static str,
    difficulty: u32,
    random: &'static str,
    css_load: bool,
    /// Never accept an answer (to check rynx gives up)
    stubborn: bool,
    state: Arc<Mutex<State>>,
}

fn cookie(request: &Request, name: &str) -> Option<String> {
    let header = request.headers.get("cookie")?.to_str().ok()?;
    header.split("; ").find_map(|c| c.strip_prefix(&format!("{name}=")).map(str::to_string))
}

fn query(request: &Request) -> HashMap<String, String> {
    request.url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect()
}

fn leading_zero_bits(hash: &[u8], bits: u32) -> bool {
    (0..bits as usize).all(|i| hash[i / 8] & (0x80 >> (i % 8)) == 0)
}

fn sha256(data: &[u8]) -> Vec<u8> {
    ring::digest::digest(&ring::digest::SHA256, data).as_ref().to_vec()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap()).collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl FakeAnubis {
    /// Check an answer as Anubis does
    fn valid(&self, issued: &Issued, q: &HashMap<String, String>) -> bool {
        if self.algorithm == "metarefresh" {
            return q.get("challenge") == Some(&issued.random)
                && issued.at.elapsed().as_millis() >= 800 * self.difficulty as u128;
        }
        let (Some(response), Some(nonce)) = (q.get("response"), q.get("nonce")) else { return false };
        if q.get("elapsedTime").and_then(|t| t.parse::<f64>().ok()).is_none() {
            return false;
        }
        let nonce: u64 = nonce.parse().unwrap();
        match self.algorithm {
            "fast" => {
                let expected = hex(&sha256(format!("{}{}", issued.random, nonce).as_bytes()));
                *response == expected && expected.starts_with(&"0".repeat(self.difficulty as usize))
            }
            "sha256" | "argon2id" => {
                let data = unhex(&issued.random);
                let le = *data.last().unwrap() >= 128;
                let hash = if self.algorithm == "sha256" {
                    let n = nonce as u32;
                    sha256(&[data.as_slice(), &if le { n.to_le_bytes() } else { n.to_be_bytes() }].concat())
                } else {
                    let salt = if le { nonce.to_le_bytes() } else { nonce.to_be_bytes() };
                    let mut out = [0u8; 32];
                    argon2::Argon2::default().hash_password_into(&data, &salt, &mut out).unwrap();
                    out.to_vec()
                };
                *response == hex(&hash) && leading_zero_bits(&hash, self.difficulty)
            }
            "hashx" => {
                // As Anubis's anubis_validate: HashX seeded with the data (a counter appended to
                // seeds HashX rejects), the 32-bit nonce hashed
                let data = unhex(&issued.random);
                let function = hashx::HashX::new(&data).unwrap_or_else(|_| {
                    (0u32..)
                        .find_map(|n| hashx::HashX::new(&[data.as_slice(), &n.to_le_bytes()].concat()).ok())
                        .unwrap()
                });
                let Ok(nonce) = u32::try_from(nonce) else { return false };
                let hash = function.hash_to_bytes(u64::from(nonce));
                *response == hex(&hash) && leading_zero_bits(&hash, self.difficulty)
            }
            _ => false,
        }
    }

    fn challenge_page(&self, id: &str) -> String {
        let extensions = if self.css_load { r#"["css-load"]"# } else { "[]" };
        let style = if self.css_load { format!(r#"<style>@import url("{CSS}{id}");</style>"#) } else { String::new() };
        format!(
            r#"<html><head><title>Making sure you're not a bot!</title>{style}
<script id="anubis_version" type="application/json">"v1.28.2"</script>
<script id="anubis_challenge" type="application/json">{{"rules":{{"algorithm":"{alg}","difficulty":{d}}},"challenge":{{"id":"{id}","method":"{alg}","randomData":"{random}","extensions":{extensions}}}}}</script>
<script id="anubis_base_prefix" type="application/json">""</script>
<script id="anubis_public_url" type="application/json">""</script>
</head><body><a href="/.within.website/x/cmd/anubis/api/honeypot/abc/init" style="display:none">trap</a>
<h1>Making sure you're not a bot!</h1><noscript>Enable JavaScript</noscript></body></html>"#,
            alg = self.algorithm,
            d = self.difficulty,
            random = self.random,
        )
    }
}

impl Respond for FakeAnubis {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let mut state = self.state.lock().unwrap();
        let path = request.url.path().to_string();
        state.paths.push(path.clone());
        if let Some(id) = path.strip_prefix(CSS) {
            state.css_fetched.push(id.to_string());
            return ResponseTemplate::new(200).set_body_raw("", "text/css");
        }
        if path == API {
            let q = query(request);
            let id = q.get("id").cloned().unwrap_or_default();
            // The test cookie must come back, and match the challenge
            if cookie(request, "techaro.lol-anubis-cookie-verification").as_deref() != Some(id.as_str()) {
                return ResponseTemplate::new(400).set_body_string("cookies disabled");
            }
            let accepted = !self.stubborn
                && state.issued.get(&id).is_some_and(|issued| self.valid(issued, &q))
                && (!self.css_load || state.css_fetched.contains(&id));
            if !accepted {
                return ResponseTemplate::new(403).set_body_string("invalid response");
            }
            state.issued.remove(&id);
            state.passes += 1;
            return ResponseTemplate::new(302)
                .append_header("set-cookie", "techaro.lol-anubis-cookie-verification=; Path=/; Max-Age=-1")
                .append_header("set-cookie", "techaro.lol-anubis-auth=signed-jwt; Path=/; HttpOnly")
                .insert_header("location", q["redir"].as_str());
        }
        if cookie(request, "techaro.lol-anubis-auth").as_deref() == Some("signed-jwt") {
            return ResponseTemplate::new(200)
                .set_body_raw(format!("<title>Chapter 1</title><p>The real page at {path}</p>"), "text/html");
        }
        let id = format!("challenge-{}", state.issued.len() + 1);
        state.issued.insert(id.clone(), Issued { random: self.random.to_string(), at: Instant::now() });
        ResponseTemplate::new(200)
            .append_header("set-cookie", format!("techaro.lol-anubis-cookie-verification={id}; Path=/").as_str())
            .append_header("set-cookie", "tracking=please; Path=/")
            .set_body_raw(self.challenge_page(&id), "text/html")
    }
}

async fn guarded_site(
    algorithm: &'static str,
    difficulty: u32,
    random: &'static str,
    css_load: bool,
    stubborn: bool,
) -> (MockServer, Arc<Mutex<State>>) {
    let server = MockServer::start().await;
    let state = Arc::new(Mutex::new(State::default()));
    Mock::given(wiremock::matchers::any())
        .respond_with(FakeAnubis { algorithm, difficulty, random, css_load, stubborn, state: Arc::clone(&state) })
        .mount(&server)
        .await;
    (server, state)
}

fn test_app() -> App {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(tx, rx).unwrap();
    app.clipboard = None;
    app.render_in_background = false;
    app
}

/// Load `url` and handle messages until the page and any status after it have arrived
async fn load(app: &mut App, url: &str) {
    app.current_tab().url_input = url.to_string();
    app.submit_request();
    let mut loaded = false;
    loop {
        let wait = if loaded { 300 } else { 120_000 };
        let Ok(Some(resp)) = tokio::time::timeout(std::time::Duration::from_millis(wait), app.rx.recv()).await else {
            assert!(loaded, "the page never loaded");
            return;
        };
        loaded |= matches!(resp, NetworkResponse::Success(..) | NetworkResponse::Error(..));
        handle_network_event(app, resp, 80).unwrap();
    }
}

fn text(app: &mut App) -> String {
    app.current_tab().rendered_content.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")
}

#[tokio::test]
async fn each_method_is_passed_without_javascript() {
    let cases = [
        ("fast", 3, "3f9a7c0d2e1b", false),
        ("fast", 2, "77aa01", true),
        ("sha256", 12, "0a1b2c3d4e5f6071", false),
        // The last byte picks the nonce's byte order
        ("sha256", 10, "0a1b2c3d4e5f60f1", false),
        ("argon2id", 1, "c0ffee00deadbeef", false),
        ("hashx", 10, "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08", false),
        ("metarefresh", 1, "5eed5eed", false),
    ];
    for (algorithm, difficulty, random, css_load) in cases {
        let (server, state) = guarded_site(algorithm, difficulty, random, css_load, false).await;
        let mut app = test_app();
        load(&mut app, &format!("{}/fiction/42", server.uri())).await;
        let page = text(&mut app);
        assert!(page.contains("The real page at /fiction/42"), "{algorithm}: {page}");
        assert_eq!(app.current_tab().page_title, "Chapter 1");

        {
            let state = state.lock().unwrap();
            assert_eq!(state.passes, 1, "{algorithm}");
            assert!(!state.paths.iter().any(|p| p.contains("honeypot")), "the trap link is never fetched");
            if css_load {
                assert_eq!(state.css_fetched.len(), 1);
            }
        }

        // The pass is remembered: the next page loads without another check
        load(&mut app, &format!("{}/fiction/43", server.uri())).await;
        assert!(text(&mut app).contains("The real page at /fiction/43"));
        assert_eq!(pass_requests(&server).await, 1, "{algorithm}");
        // Only Anubis's cookies are kept: the site's other cookie is never sent back
        for request in server.received_requests().await.unwrap() {
            assert!(cookie(&request, "tracking").is_none(), "{algorithm}: a non-Anubis cookie was kept");
        }
    }
}

/// How many answers were handed in
async fn pass_requests(server: &MockServer) -> usize {
    let requests = server.received_requests().await.unwrap();
    requests.iter().filter(|r| r.url.path() == API).count()
}

#[tokio::test]
async fn a_check_that_keeps_refusing_is_shown_not_retried_forever() {
    let (server, state) = guarded_site("fast", 1, "abcd", false, true).await;
    let mut app = test_app();
    load(&mut app, &format!("{}/", server.uri())).await;
    assert!(app.current_tab().status_message.contains("bot check"), "{}", app.current_tab().status_message);
    // One answer, then the refusal page is shown rather than trying again
    assert!(state.lock().unwrap().paths.iter().filter(|p| *p == API).count() <= 1);
}

#[tokio::test]
async fn unsupported_methods_and_the_setting_show_the_check() {
    let (server, state) = guarded_site("equix", 4, "abcd", false, false).await;
    let mut app = test_app();
    load(&mut app, &format!("{}/", server.uri())).await;
    assert!(app.current_tab().status_message.contains("equix"), "{}", app.current_tab().status_message);
    assert!(text(&mut app).contains("Making sure you're not a bot"));
    assert_eq!(state.lock().unwrap().passes, 0);

    let (server, state) = guarded_site("fast", 1, "abcd", false, false).await;
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let (settings, _) = rynx::settings::Settings::parse("anubis = off");
    let mut app = App::with_settings(tx, rx, settings).unwrap();
    app.clipboard = None;
    app.render_in_background = false;
    load(&mut app, &format!("{}/", server.uri())).await;
    assert!(app.current_tab().status_message.contains("turned off"), "{}", app.current_tab().status_message);
    assert!(state.lock().unwrap().paths.iter().all(|p| p != API));
}

#[tokio::test]
async fn cloudflare_checks_are_explained() {
    // Cloudflare's check needs JavaScript, so rynx says what it is and where to go instead
    let server = MockServer::builder().start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(
            ResponseTemplate::new(403).insert_header("cf-mitigated", "challenge").set_body_raw(
                "<title>Just a moment...</title><p>Enable JavaScript and cookies to continue",
                "text/html",
            ),
        )
        .mount(&server)
        .await;
    let mut app = test_app();
    load(&mut app, &format!("{}/", server.uri())).await;
    let status = app.current_tab().status_message.clone();
    assert!(status.contains("Cloudflare") && status.contains("Internet Archive"), "{status}");
    assert!(text(&mut app).contains("Enable JavaScript"));
}

#[test]
fn challenges_are_read_from_the_page() {
    let page = r#"<script id="anubis_challenge" type="application/json">{"rules":{"algorithm":"fast","difficulty":4},"challenge":"0123abcd"}</script>
                  <script id="anubis_base_prefix" type="application/json">"/myprefix"</script>"#;
    let challenge = anubis::detect(page).unwrap();
    assert_eq!(challenge.id, "", "older versions have no id");
    assert_eq!(challenge.random_data, "0123abcd");
    assert_eq!(challenge.base_prefix, "/myprefix");
    let address = anubis::pass_address(
        "https://site.example/a?b=c",
        &challenge,
        Some(&anubis::Answer { response: "00ab".into(), nonce: 7 }),
        std::time::Duration::from_millis(1500),
    )
    .unwrap();
    assert_eq!(
        address,
        "https://site.example/myprefix/.within.website/x/cmd/anubis/api/pass-challenge?response=00ab&nonce=7&redir=https%3A%2F%2Fsite.example%2Fa%3Fb%3Dc&elapsedTime=1500"
    );

    assert!(anubis::detect("<p>An ordinary page</p>").is_none());
    assert!(anubis::detect("<p>A page about anubis_challenge</p>").is_none());
    // A base prefix can't send the answer to another site
    let evil = Challenge { base_prefix: "//evil.example".into(), ..challenge };
    assert!(anubis::pass_address("https://site.example/", &evil, None, Default::default()).is_err());
}

#[test]
fn impossible_difficulties_give_up() {
    let challenge = Challenge {
        id: String::new(),
        random_data: "ab".into(),
        algorithm: "nonsense".into(),
        difficulty: 4,
        base_prefix: String::new(),
        public_url: String::new(),
        stylesheets: Vec::new(),
    };
    assert!(anubis::solve(&challenge).unwrap_err().contains("nonsense"));
}

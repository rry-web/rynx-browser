//! ALTCHA proof-of-work checks, passed without JavaScript, against a fake site that checks
//! answers the way the page's own script hands them in (the page modelled on one in use).

use rynx::app::App;
use rynx::event_handler::handle_network_event;
use rynx::network::NetworkResponse;
use std::sync::{Arc, Mutex};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const SALT: &str = "824447c837f68ca27b82d122";
const SIGNATURE: &str = "267105e075cd78ae94467de26f9049f8795421bd1fc1c78b0d26625b32215617";

fn sha256_hex(data: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, data).as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

fn unbase64(text: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let values: Vec<u32> =
        text.bytes().filter(|b| *b != b'=').map(|b| ALPHABET.iter().position(|a| *a == b).unwrap() as u32).collect();
    let mut out = Vec::new();
    for chunk in values.chunks(4) {
        let n = chunk.iter().enumerate().fold(0, |n, (i, v)| n | (v << (18 - 6 * i)));
        for i in 0..chunk.len() - 1 {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    out
}

fn cookie(request: &Request, name: &str) -> Option<String> {
    let header = request.headers.get("cookie")?.to_str().ok()?;
    header.split("; ").find_map(|c| c.strip_prefix(&format!("{name}=")).map(str::to_string))
}

#[derive(Default)]
struct State {
    answers: usize,
    passes: usize,
}

/// A site guarding every page with an ALTCHA challenge whose answer is `number`
struct FakeSite {
    number: u64,
    /// Never accept an answer (to check rynx gives up)
    stubborn: bool,
    state: Arc<Mutex<State>>,
}

impl FakeSite {
    fn challenge(&self) -> String {
        sha256_hex(format!("{SALT}{}", self.number).as_bytes())
    }

    fn page(&self) -> String {
        format!(
            r#"<!DOCTYPE html><html><head><title>Verifying…</title></head><body><h1>One moment — verifying your browser</h1><p>Solving a quick proof-of-work to filter automated traffic.</p><script>(function(){{var CH={{"algorithm":"SHA-256","challenge":"{}","maxnumber":200000,"salt":"{SALT}","signature":"{SIGNATURE}"}};var w=new Worker(u);w.onmessage=function(e){{var sol={{algorithm:CH.algorithm,challenge:CH.challenge,number:e.data.n,salt:CH.salt,signature:CH.signature}};var body=btoa(JSON.stringify(sol));fetch("/api/challenge",{{method:"POST",headers:{{"Content-Type":"application/json"}},body:JSON.stringify({{payload:body,st:st}})}}).then(function(r){{return r.json()}}).then(function(j){{if(j&&j.ok&&j.dest){{location.replace(j.dest)}}}})}};}})();</script></body></html>"#,
            self.challenge()
        )
    }
}

impl Respond for FakeSite {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let mut state = self.state.lock().unwrap();
        let path = request.url.path().to_string();
        if path == "/api/challenge" && request.method.as_str() == "POST" {
            state.answers += 1;
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            let solution: serde_json::Value =
                serde_json::from_slice(&unbase64(body["payload"].as_str().unwrap())).unwrap();
            let number = solution["number"].as_u64().unwrap();
            let accepted = !self.stubborn
                && body["st"].is_u64()
                && solution["algorithm"] == "SHA-256"
                && solution["salt"] == SALT
                && solution["signature"] == SIGNATURE
                && solution["challenge"] == self.challenge()
                && sha256_hex(format!("{SALT}{number}").as_bytes()) == self.challenge();
            if !accepted {
                return ResponseTemplate::new(200)
                    .set_body_raw(r#"{"ok":false,"error":"Verification rejected."}"#, "application/json");
            }
            state.passes += 1;
            return ResponseTemplate::new(200)
                .append_header("set-cookie", "tf_pass=signed; Path=/; HttpOnly")
                .set_body_raw(r#"{"ok":true,"dest":"/papers/42"}"#, "application/json");
        }
        if cookie(request, "tf_pass").as_deref() == Some("signed") {
            return ResponseTemplate::new(200)
                .set_body_raw(format!("<title>Paper</title><p>The real page at {path}</p>"), "text/html");
        }
        ResponseTemplate::new(200)
            .append_header("set-cookie", "tracking=please; Path=/")
            .set_body_raw(self.page(), "text/html")
    }
}

async fn guarded_site(number: u64, stubborn: bool) -> (MockServer, Arc<Mutex<State>>) {
    let server = MockServer::start().await;
    let state = Arc::new(Mutex::new(State::default()));
    Mock::given(wiremock::matchers::any())
        .respond_with(FakeSite { number, stubborn, state: Arc::clone(&state) })
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
async fn the_proof_of_work_is_done_and_the_page_loads() {
    let (server, state) = guarded_site(123_456, false).await;
    let mut app = test_app();
    load(&mut app, &format!("{}/papers/42", server.uri())).await;
    assert!(text(&mut app).contains("The real page at /papers/42"), "{}", text(&mut app));
    assert_eq!(state.lock().unwrap().passes, 1);
    // The pass is kept: the next page loads without another check
    load(&mut app, &format!("{}/papers/43", server.uri())).await;
    assert!(text(&mut app).contains("The real page at /papers/43"));
    assert_eq!(state.lock().unwrap().answers, 1);
    // Only the pass is kept: the site's other cookie is never sent back
    for request in server.received_requests().await.unwrap() {
        assert!(cookie(&request, "tracking").is_none(), "a cookie other than the pass was kept");
    }
}

#[tokio::test]
async fn a_refused_answer_shows_the_check_and_is_not_retried() {
    let (server, state) = guarded_site(7, true).await;
    let mut app = test_app();
    load(&mut app, &format!("{}/", server.uri())).await;
    let status = app.current_tab().status_message.clone();
    assert!(status.contains("bot check (ALTCHA)") && status.contains("Verification rejected"), "{status}");
    assert!(text(&mut app).contains("verifying your browser"));
    assert_eq!(state.lock().unwrap().answers, 1);
}

#[test]
fn challenges_are_read_from_the_page() {
    let site = FakeSite { number: 5, stubborn: false, state: Default::default() };
    let challenge = rynx::altcha::detect(&site.page()).expect("a challenge");
    assert_eq!(challenge.algorithm, "SHA-256");
    assert_eq!(challenge.max_number, 200_000);
    assert_eq!(challenge.answer_path, "/api/challenge");
    assert_eq!(rynx::altcha::solve(&challenge), Ok(5));
    // Not a challenge: no signed fields, or nowhere the answer is posted
    assert!(rynx::altcha::detect("<p>\"maxnumber\" in a sentence</p>").is_none());
    assert!(rynx::altcha::detect(&site.page().replace("method:\"POST\"", "method:\"GET\"")).is_none());
    // An answer beyond the limit isn't searched for forever
    let unsolvable = rynx::altcha::Challenge { max_number: 4, ..challenge };
    assert!(rynx::altcha::solve(&unsolvable).is_err());
}

//! I2P mode, against a fake I2P proxy (I2P-01/03/04, CACHE-02, DL-08 and SET-08's I2P part in
//! TESTING.md)

use super::harness::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::{Arc, Mutex};

/// A request the proxy received: its first line ("GET http://site.i2p/ HTTP/1.1") and headers
#[derive(Clone, Debug)]
struct Seen {
    line: String,
    headers: Vec<(String, String)>,
}

impl Seen {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// What the proxy answers: the status, extra headers, the body, and how many bytes of it to send
/// before cutting the connection (None: all of it)
struct Answer {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    cut_after: Option<usize>,
}

impl Answer {
    fn page(html: &str) -> Answer {
        Answer {
            status: 200,
            headers: vec![("Content-Type".into(), "text/html".into())],
            body: html.as_bytes().to_vec(),
            cut_after: None,
        }
    }
    fn status(status: u16, html: &str) -> Answer {
        Answer { status, ..Answer::page(html) }
    }
}

type Router = Arc<dyn Fn(&Seen) -> Answer + Send + Sync>;

/// A fake I2P router's HTTP proxy, answering with `router` and recording every request
struct Proxy {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Proxy {
    fn start(router: Router) -> Proxy {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let (router, record) = (Arc::clone(&router), Arc::clone(&record));
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    let mut headers = Vec::new();
                    let mut length = 0;
                    loop {
                        let mut header = String::new();
                        if reader.read_line(&mut header).unwrap_or(0) == 0 || header.trim().is_empty() {
                            break;
                        }
                        if let Some((name, value)) = header.trim().split_once(':') {
                            if name.eq_ignore_ascii_case("content-length") {
                                length = value.trim().parse().unwrap_or(0);
                            }
                            headers.push((name.trim().to_string(), value.trim().to_string()));
                        }
                    }
                    let mut body = vec![0u8; length];
                    let _ = reader.read_exact(&mut body);
                    let request = Seen { line: line.trim().to_string(), headers };
                    record.lock().unwrap().push(request.clone());
                    let answer = router(&request);
                    let mut head = format!(
                        "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                        answer.status,
                        answer.body.len()
                    );
                    for (name, value) in &answer.headers {
                        head.push_str(&format!("{name}: {value}\r\n"));
                    }
                    head.push_str("\r\n");
                    let _ = stream.write_all(head.as_bytes());
                    let sent = answer.cut_after.unwrap_or(answer.body.len()).min(answer.body.len());
                    let _ = stream.write_all(&answer.body[..sent]);
                    let _ = stream.flush();
                });
            }
        });
        Proxy { port, seen }
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    fn requests_for(&self, url: &str) -> usize {
        self.seen().iter().filter(|s| s.line.contains(&format!(" {url} "))).count()
    }
}

fn in_i2p_mode(name: &str, proxy: &Proxy, settings: &str) -> (Home, Rynx) {
    let home = Home::new(name);
    home.settings(&format!("i2p_proxy = http://127.0.0.1:{}\n{settings}", proxy.port));
    let mut rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    rynx.keys("p");
    rynx.wait_for("I2P mode on");
    (home, rynx)
}

/// I2P-01, I2P-04: in I2P mode pages go through the proxy, with the generic identity; p again
/// and the clearweb is reached directly
#[test]
fn i2p_01_04_browsing_through_the_proxy_and_back() {
    let proxy = Proxy::start(Arc::new(|seen: &Seen| {
        if seen.line.starts_with("GET http://site.i2p/ ") {
            Answer::page("<title>Eepsite</title><h1>An I2P site</h1>")
        } else {
            Answer::status(500, "<h1>Proxy error: Host not found</h1>")
        }
    }));
    let site = Site::new(&[("/", "<h1>A clearweb page</h1>")]);
    let (_home, mut rynx) = in_i2p_mode("i2p-browse", &proxy, "");
    rynx.open("http://site.i2p/");
    rynx.wait_for("An I2P site");
    rynx.wait_for("I2P mode");
    let request =
        proxy.seen().into_iter().find(|s| s.line.starts_with("GET http://site.i2p/ ")).expect("not through the proxy");
    assert_eq!(request.header("user-agent"), Some("MYOB/6.66 (AN/ON)"));
    assert!(request.header("referer").is_none());
    rynx.keys("p");
    rynx.wait_for("I2P mode off");
    let before = proxy.seen().len();
    rynx.open(&site.url("/"));
    rynx.wait_for("A clearweb page");
    rynx.wait_for("Clearweb");
    assert_eq!(proxy.seen().len(), before, "the clearweb page went through the I2P proxy");
    assert_eq!(site.requests("/"), 1);
    rynx.quit();
}

/// I2P-03: an address the router doesn't know is looked up with the jump services in turn
#[test]
fn i2p_03_jump_services() {
    let proxy = Proxy::start(Arc::new(|seen: &Seen| {
        let line = &seen.line;
        if line.starts_with("GET http://i2p-projekt.i2p/jump/newsite.i2p ") {
            Answer::status(500, "<h1>Jump service unreachable</h1>")
        } else if line.starts_with("GET http://stats.i2p/jump/newsite.i2p ") {
            Answer::page("<title>New site</title><h1>Found through a jump service</h1>")
        } else {
            Answer::status(500, "<h1>Proxy error: Host not found</h1>")
        }
    }));
    let (_home, mut rynx) = in_i2p_mode("i2p-jump", &proxy, "");
    rynx.open("http://newsite.i2p/");
    rynx.wait_for("Found through a jump service");
    assert_eq!(proxy.requests_for("http://i2p-projekt.i2p/jump/newsite.i2p"), 1);
    assert_eq!(proxy.requests_for("http://stats.i2p/jump/newsite.i2p"), 1);
    assert_eq!(proxy.requests_for("http://reg.i2p/jump/newsite.i2p"), 0, "kept going after one answered");
    rynx.quit();
}

/// CACHE-02: an I2P site that can't be reached later shows the copy from the first visit
#[test]
fn cache_02_i2p_site_gone() {
    let up = Arc::new(Mutex::new(true));
    let state = Arc::clone(&up);
    let proxy = Proxy::start(Arc::new(move |seen: &Seen| {
        if seen.line.starts_with("GET http://site.i2p/ ") && *state.lock().unwrap() {
            Answer::page("<title>Eepsite</title><h1>The first visit's copy</h1>")
        } else {
            Answer::status(504, "<h1>Proxy error: Not reachable</h1>")
        }
    }));
    let (_home, mut rynx) = in_i2p_mode("i2p-gone", &proxy, "");
    rynx.open("http://site.i2p/");
    rynx.wait_for("The first visit's copy");
    rynx.keys("n");
    *up.lock().unwrap() = false;
    rynx.open("http://site.i2p/");
    rynx.wait_for("This is the copy saved");
    rynx.wait_for("The first visit's copy");
    assert!(!rynx.text().contains("Not reachable"));
    rynx.quit();
}

/// DL-08: a download over I2P that's cut off resumes where it stopped
#[test]
fn dl_08_resuming_over_i2p() {
    let file: Vec<u8> = (0..400_000u32).map(|i| (i % 253) as u8).collect();
    let whole = file.clone();
    let proxy = Proxy::start(Arc::new(move |seen: &Seen| {
        if seen.line.starts_with("GET http://files.i2p/big.zip ") {
            let mut headers = vec![
                ("Content-Type".to_string(), "application/zip".to_string()),
                ("Accept-Ranges".into(), "bytes".into()),
            ];
            match seen
                .header("range")
                .and_then(|r| r.strip_prefix("bytes="))
                .and_then(|r| r.trim_end_matches('-').parse::<usize>().ok())
            {
                Some(from) => {
                    headers
                        .push(("Content-Range".into(), format!("bytes {}-{}/{}", from, whole.len() - 1, whole.len())));
                    Answer { status: 206, headers, body: whole[from..].to_vec(), cut_after: None }
                }
                // The first try is cut off partway
                None => Answer { status: 200, headers, body: whole.clone(), cut_after: Some(150_000) },
            }
        } else if seen.line.starts_with("GET http://files.i2p/ ") {
            Answer::page("<h1>Files</h1><p><a href=\"/big.zip\">The big file</a></p>")
        } else {
            Answer::status(500, "<h1>Proxy error</h1>")
        }
    }));
    let (home, mut rynx) = in_i2p_mode("i2p-resume", &proxy, "");
    rynx.open("http://files.i2p/");
    rynx.wait_for("The big file");
    rynx.keys("d");
    rynx.wait_for("big.zip");
    rynx.keys("y");
    home.wait_for_file(&home.downloads().join("big.zip"), &file);
    let ranges: Vec<String> = proxy.seen().iter().filter_map(|s| s.header("range").map(str::to_string)).collect();
    assert!(!ranges.is_empty(), "the download wasn't resumed: it started again");
    rynx.quit();
}

/// SET-08 (I2P): with saved history, an I2P visit is listed until you quit, never saved
#[test]
fn set_08_i2p_visits_never_reach_history_txt() {
    let proxy = Proxy::start(Arc::new(|seen: &Seen| {
        if seen.line.starts_with("GET http://diary.i2p/ ") {
            Answer::page("<title>Secret diary</title><h1>An I2P diary</h1>")
        } else {
            Answer::status(500, "<h1>Proxy error</h1>")
        }
    }));
    let (home, mut rynx) = in_i2p_mode("i2p-history", &proxy, "history = on\n");
    rynx.open("http://diary.i2p/");
    rynx.wait_for("An I2P diary");
    rynx.keys("s");
    rynx.wait_for("Secret diary");
    rynx.quit();
    let file = home.read_config("history.txt").unwrap_or_default();
    assert!(!file.contains("diary.i2p") && !file.contains("Secret diary"), "an I2P visit was saved: {file}");
}

/// I2P-02: https:// addresses in I2P mode go through the I2P proxy too (as CONNECT, to its
/// outproxy), never straight to the clearweb; nor do searches typed into the address bar
#[test]
fn i2p_02_https_goes_through_the_proxy() {
    let proxy = Proxy::start(Arc::new(|_: &Seen| Answer::status(503, "<h1>No outproxy</h1>")));
    let (_home, mut rynx) = in_i2p_mode("i2p-https", &proxy, "");
    rynx.open("https://clearweb.example/page");
    std::thread::sleep(std::time::Duration::from_secs(2));
    assert!(
        proxy.seen().iter().any(|s| s.line.starts_with("CONNECT clearweb.example:443 ")),
        "https:// in I2P mode didn't go through the I2P proxy: {:?}",
        proxy.seen()
    );
    rynx.open("some search words");
    std::thread::sleep(std::time::Duration::from_secs(2));
    assert!(
        proxy.seen().iter().any(|s| s.line.starts_with("CONNECT ") && s.line.contains("marginalia")),
        "a search left I2P: {:?}",
        proxy.seen()
    );
    rynx.quit();
}

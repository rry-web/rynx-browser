//! Resuming downloads: by itself when the connection breaks, and later from what was kept.

use rynx::app::App;
use rynx::event_handler::handle_network_event;
use rynx::network::NetworkResponse;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// The file being downloaded: 1000 bytes that differ by position, so a splice would show
fn contents(version: u8) -> Vec<u8> {
    (0..1000u32).map(|i| (i % 251) as u8 ^ version).collect()
}

/// A server that answers each connection with `respond(connection number, request)`, as raw
/// bytes, then closes it (sending less than promised simulates a broken connection)
async fn scripted_server(
    respond: impl Fn(usize, &str) -> Vec<u8> + Send + Sync + 'static,
) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}/file.bin", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&requests);
    tokio::spawn(async move {
        for connection in 0.. {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let mut buffer = vec![0u8; 8192];
            let n = socket.read(&mut buffer).await.unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..n]).to_lowercase();
            seen.lock().unwrap().push(request.clone());
            let _ = socket.write_all(&respond(connection, &request)).await;
            let _ = socket.shutdown().await;
        }
    });
    (address, requests)
}

fn response(status: &str, headers: &[(&str, String)], body: &[u8], promised: usize) -> Vec<u8> {
    let mut text = format!("HTTP/1.1 {}\r\ncontent-length: {}\r\nconnection: close\r\n", status, promised);
    for (name, value) in headers {
        text.push_str(&format!("{}: {}\r\n", name, value));
    }
    text.push_str("\r\n");
    [text.as_bytes(), body].concat()
}

/// Answers a range request ("range: bytes=N-") with the rest of `file`, if `etag` matches
/// If-Range (or there's none); otherwise the whole file
fn ranged(file: &[u8], etag: &str, request: &str) -> Vec<u8> {
    let start = request
        .lines()
        .find_map(|l| l.strip_prefix("range: bytes="))
        .and_then(|r| r.trim_end_matches('-').trim().parse::<usize>().ok());
    let if_range_ok =
        request.lines().find_map(|l| l.strip_prefix("if-range: ")).is_none_or(|v| v.trim() == etag.to_lowercase());
    let headers = vec![("etag", etag.to_string()), ("accept-ranges", "bytes".to_string())];
    match start {
        Some(start) if if_range_ok => {
            let mut headers = headers;
            headers.push(("content-range", format!("bytes {}-{}/{}", start, file.len() - 1, file.len())));
            response("206 Partial Content", &headers, &file[start..], file.len() - start)
        }
        _ => response("200 OK", &headers, file, file.len()),
    }
}

fn test_app() -> App {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(tx, rx).unwrap();
    app.clipboard = None;
    app.render_in_background = false;
    app.download_dir = std::env::temp_dir().join(format!("rynx-resume-{}-{}", std::process::id(), rand_suffix()));
    app
}

fn rand_suffix() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

/// Download `url` through the prompt, and wait for the end; returns the result and status lines
async fn download(app: &mut App, url: &str) -> (NetworkResponse, Vec<String>) {
    let dir = app.download_dir.clone();
    app.current_tab().initiate_download_request(url.to_string(), &dir);
    let prompt = app.current_tab().download_prompt.take().expect("prompt shown");
    app.trigger_download(prompt);
    let mut statuses = Vec::new();
    loop {
        let resp = tokio::time::timeout(std::time::Duration::from_secs(30), app.rx.recv()).await.unwrap().unwrap();
        match resp {
            NetworkResponse::DownloadFinished(..) | NetworkResponse::DownloadError(..) => return (resp, statuses),
            NetworkResponse::Info(_, ref text) => {
                statuses.push(text.clone());
                handle_network_event(app, resp, 80).unwrap();
            }
            other => handle_network_event(app, other, 80).unwrap(),
        }
    }
}

fn saved(resp: &NetworkResponse) -> PathBuf {
    match resp {
        NetworkResponse::DownloadFinished(_, path) => PathBuf::from(path),
        NetworkResponse::DownloadError(_, e) => panic!("download failed: {e}"),
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn a_broken_connection_is_resumed_where_it_stopped() {
    let file = contents(0);
    let first_part = file[..400].to_vec();
    let whole = file.clone();
    let (url, requests) = scripted_server(move |connection, request| {
        if connection == 0 {
            // Promises 1000 bytes, sends 400, and hangs up
            let headers = [("etag", "\"v1\"".to_string()), ("accept-ranges", "bytes".to_string())];
            response("200 OK", &headers, &first_part, 1000)
        } else {
            ranged(&whole, "\"v1\"", request)
        }
    })
    .await;
    let mut app = test_app();
    let (resp, statuses) = download(&mut app, &url).await;
    let path = saved(&resp);
    assert_eq!(std::fs::read(&path).unwrap(), file, "the two parts join into the whole file");
    assert!(statuses.iter().any(|s| s.contains("resuming, attempt 1")), "{statuses:?}");
    let requests = requests.lock().unwrap();
    assert!(requests[1].contains("range: bytes=400-") && requests[1].contains("if-range: \"v1\""), "{}", requests[1]);
    assert!(!path.with_file_name("file.bin.part").exists(), "no partial file is left");
    let _ = std::fs::remove_dir_all(&app.download_dir);
}

/// Leave a partial download behind, as an earlier session would have
fn leave_partial(app: &App, url: &str, part: &[u8], validator: &str) {
    std::fs::create_dir_all(&app.download_dir).unwrap();
    std::fs::write(app.download_dir.join("file.bin.part"), part).unwrap();
    let note = serde_json::json!({ "url": url, "validator": validator });
    std::fs::write(app.download_dir.join("file.bin.part.json"), note.to_string()).unwrap();
}

#[tokio::test]
async fn a_download_interrupted_earlier_resumes_on_the_next_try() {
    let file = contents(0);
    let whole = file.clone();
    let (url, requests) = scripted_server(move |_, request| ranged(&whole, "\"v1\"", request)).await;
    let mut app = test_app();
    leave_partial(&app, &url, &file[..300], "\"v1\"");

    // The prompt says so
    let dir = app.download_dir.clone();
    app.current_tab().initiate_download_request(url.clone(), &dir);
    assert_eq!(app.current_tab().download_prompt.as_ref().unwrap().resume_from, Some(300));
    app.current_tab().download_prompt = None;

    let (resp, _) = download(&mut app, &url).await;
    assert_eq!(std::fs::read(saved(&resp)).unwrap(), file);
    assert!(requests.lock().unwrap()[0].contains("range: bytes=300-"));
    let _ = std::fs::remove_dir_all(&app.download_dir);
}

#[tokio::test]
async fn a_file_changed_on_the_server_is_downloaded_afresh() {
    let old = contents(0);
    let new = contents(7);
    let served = new.clone();
    // The server's file is now "v2": If-Range "v1" doesn't match, so it sends all of it
    let (url, _) = scripted_server(move |_, request| ranged(&served, "\"v2\"", request)).await;
    let mut app = test_app();
    leave_partial(&app, &url, &old[..500], "\"v1\"");
    let (resp, _) = download(&mut app, &url).await;
    assert_eq!(std::fs::read(saved(&resp)).unwrap(), new, "never half old, half new");
    let _ = std::fs::remove_dir_all(&app.download_dir);
}

#[tokio::test]
async fn a_partial_download_of_another_address_is_not_used() {
    let file = contents(0);
    let whole = file.clone();
    let (url, requests) = scripted_server(move |_, request| ranged(&whole, "\"v1\"", request)).await;
    let mut app = test_app();
    leave_partial(&app, "https://elsewhere.example/file.bin", &contents(9)[..300], "\"v1\"");
    let (resp, _) = download(&mut app, &url).await;
    assert_eq!(std::fs::read(saved(&resp)).unwrap(), file);
    assert!(!requests.lock().unwrap()[0].contains("range:"));
    let _ = std::fs::remove_dir_all(&app.download_dir);
}

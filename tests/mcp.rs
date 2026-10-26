//! MCP tools: listing a site's tools as forms, confirming calls, and showing results.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::backend::TestBackend;
use rynx_browser::app::App;
use rynx_browser::event_handler::{handle_key_event, handle_network_event};
use rynx_browser::forms::ControlValue;
use rynx_browser::mcp::{self, check_server};
use rynx_browser::network::{NetworkResponse, PageKind};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

#[test]
fn servers_must_belong_to_the_page() {
    let page = Some("https://shop.example/products/1");
    assert_eq!(check_server("/mcp", page, false).unwrap(), "https://shop.example/mcp");
    assert_eq!(check_server("https://api.shop.example/mcp", page, false).unwrap(), "https://api.shop.example/mcp");
    assert_eq!(
        check_server("https://shop.example/mcp", Some("https://www.shop.example/"), false).unwrap(),
        "https://shop.example/mcp"
    );
    assert!(check_server("http://shop.example/mcp", page, false).unwrap().starts_with("https://"), "upgraded to HTTPS");
    assert!(check_server("https://evil.example/mcp", page, false).unwrap_err().contains("another site"));
    assert!(check_server("https://shop.example.evil.example/mcp", page, false).is_err());
    assert!(check_server("https://user@shop.example/mcp", page, false).is_err());
    assert!(check_server("gopher://shop.example/mcp", page, false).is_err());
    assert!(check_server("javascript:alert(1)", page, false).is_err());
    // A public page can't reach local servers, even if it's "localhost" pretending to be its site
    assert!(check_server("http://127.0.0.1:3000/mcp", Some("https://127.0.0.1.example/"), false).is_err());
    // Typed by the user: their choice
    assert_eq!(check_server("http://localhost:3000/mcp", None, false).unwrap(), "http://localhost:3000/mcp");
}

/// A read-only tool taking input described by `schema`
fn tool_with_schema(schema: Value) -> mcp::Tool {
    mcp::Tool {
        name: "t".into(),
        title: "t".into(),
        description: String::new(),
        input_schema: schema,
        read_only: true,
        destructive: false,
    }
}

#[test]
fn form_fields_become_typed_arguments() {
    use mcp::FieldValue::{Checked, Text};
    let tool = tool_with_schema(json!({
        "type": "object",
        "properties": {
            "query": { "type": "string" },
            "count": { "type": "integer" },
            "ratio": { "type": "number" },
            "exact": { "type": "boolean" },
            "sort": { "enum": ["new", "old"] },
            "level": { "enum": [1, 2] },
            "tags": { "type": "array" },
            "note": { "type": ["string", "null"] }
        },
        "required": ["query"]
    }));
    let entered = |pairs: &[(&str, mcp::FieldValue)]| pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();

    let args = mcp::arguments(
        &tool,
        &entered(&[
            ("query", Text("rust".into())),
            ("count", Text(" 5 ".into())),
            ("ratio", Text("0.5".into())),
            ("exact", Checked(true)),
            ("sort", Text("old".into())),
            ("level", Text("2".into())),
            ("tags", Text("[\"a\", \"b\"]".into())),
            ("note", Text("".into())),
        ]),
    )
    .unwrap();
    assert_eq!(
        args,
        json!({ "query": "rust", "count": 5, "ratio": 0.5, "exact": true, "sort": "old", "level": 2, "tags": ["a", "b"] })
    );

    let fails = |pairs: &[(&str, mcp::FieldValue)]| mcp::arguments(&tool, &entered(pairs)).unwrap_err();
    assert!(fails(&[]).contains("Fill in \"query\""));
    assert!(fails(&[("query", Text("x".into())), ("count", Text("five".into()))]).contains("whole number"));
    assert!(fails(&[("query", Text("x".into())), ("tags", Text("{}".into()))]).contains("JSON array"));
    assert!(fails(&[("query", Text("x".into())), ("sort", Text("sideways".into()))]).contains("listed choices"));
}

#[test]
fn results_are_shown_safely() {
    let result = json!({
        "content": [
            { "type": "text", "text": "Line <one>\nline two\n\nSecond paragraph\n  - nested" },
            { "type": "resource_link", "uri": "https://x.example/doc", "name": "The doc" },
            { "type": "image", "data": "AAAA", "mimeType": "image/png" },
            { "type": "resource", "resource": { "uri": "x", "text": "  indented" } }
        ]
    });
    assert!(mcp::tools_page_html("https://x.example/mcp", &[]).contains("offers no tools"));
    let html = mcp::result_page_html("https://x.example/mcp", "Search", &Ok(result));
    assert!(html.contains("<p>Line &lt;one&gt;<br>line two</p><p>Second paragraph<br>&nbsp;&nbsp;- nested</p>"), "{html}");
    assert!(html.contains("<a href=\"https://x.example/doc\">The doc</a>"));
    assert!(html.contains("image (image/png)"));
    assert!(html.contains("<pre>  indented</pre>"));
    assert!(html.contains(&mcp::tools_address("https://x.example/mcp")), "links back to the tools");

    let error = mcp::result_page_html("https://x.example/mcp", "Search", &Ok(json!({ "isError": true, "content": [] })));
    assert!(error.contains("reported an error"));
    let structured = mcp::result_page_html("https://x.example/mcp", "S", &Ok(json!({ "structuredContent": { "n": 1 } })));
    assert!(structured.contains("&quot;n&quot;: 1"), "{structured}");
}

// --- A fake MCP server ---

/// Answers MCP requests and records every message it receives
struct FakeServer {
    received: Arc<Mutex<Vec<Value>>>,
    /// Answer tools/list as a server-sent event stream
    stream: bool,
}

impl Respond for FakeServer {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let message: Value = serde_json::from_slice(&request.body).unwrap();
        self.received.lock().unwrap().push(message.clone());
        let session = request.headers.get("mcp-session-id").map(|v| v.to_str().unwrap().to_string());
        let id = message["id"].clone();
        let answer = |result: Value| json!({ "jsonrpc": "2.0", "id": id, "result": result });
        match message["method"].as_str().unwrap() {
            "initialize" => {
                assert_eq!(message["params"]["clientInfo"]["name"], "rynx-browser");
                ResponseTemplate::new(200)
                    .insert_header("mcp-session-id", "session-1")
                    .set_body_json(answer(json!({ "protocolVersion": "2025-06-18", "capabilities": { "tools": {} }, "serverInfo": { "name": "fake" } })))
            }
            _ if session.as_deref() != Some("session-1") => ResponseTemplate::new(400),
            "notifications/initialized" => ResponseTemplate::new(202),
            "tools/list" => {
                let tools = answer(json!({ "tools": [
                    {
                        "name": "search",
                        "title": "Search the shop",
                        "description": "Finds <products>.",
                        "annotations": { "readOnlyHint": true },
                        "inputSchema": { "type": "object", "properties": {
                            "query": { "type": "string", "description": "Words to look for" },
                            "limit": { "type": "integer", "default": 10 }
                        }, "required": ["query"] }
                    },
                    { "name": "delete_cart", "description": "Empties your cart", "inputSchema": { "type": "object" } }
                ]}));
                if self.stream {
                    let body = format!("event: message\ndata: {}\n\n", tools);
                    ResponseTemplate::new(200).set_body_raw(body, "text/event-stream")
                } else {
                    ResponseTemplate::new(200).set_body_json(tools)
                }
            }
            "tools/call" => {
                let args = &message["params"]["arguments"];
                ResponseTemplate::new(200).set_body_json(answer(json!({
                    "content": [{ "type": "text", "text": format!("Found 3 results for {}", args["query"]) }]
                })))
            }
            _ => ResponseTemplate::new(200).set_body_json(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "no such method" } })),
        }
    }
}

async fn shop(stream: bool) -> (MockServer, Arc<Mutex<Vec<Value>>>) {
    let server = MockServer::start().await;
    let received = Arc::new(Mutex::new(Vec::new()));
    Mock::given(method("POST"))
        .and(path("/mcp"))
        .respond_with(FakeServer { received: Arc::clone(&received), stream })
        .mount(&server)
        .await;
    // Opened as a page, the server refuses the way real ones do
    Mock::given(method("GET"))
        .and(path("/mcp"))
        .respond_with(ResponseTemplate::new(406).set_body_json(json!({
            "jsonrpc": "2.0", "id": "server-error",
            "error": { "code": -32600, "message": "Not Acceptable: Client must accept text/event-stream" }
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "<html><head><title>Shop</title><link rel=\"mcp\" href=\"/mcp\"></head><body><p>Welcome</p></body></html>",
            "text/html",
        ))
        .mount(&server)
        .await;
    (server, received)
}

fn methods(received: &Mutex<Vec<Value>>) -> Vec<String> {
    received.lock().unwrap().iter().map(|m| m["method"].as_str().unwrap().to_string()).collect()
}

fn test_app() -> App {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(tx, rx).unwrap();
    app.clipboard = None;
    app.render_in_background = false;
    app
}

fn press(app: &mut App, code: KeyCode) -> bool {
    let key = KeyEvent { code, modifiers: KeyModifiers::empty(), kind: KeyEventKind::Press, state: KeyEventState::empty() };
    handle_key_event(app, key, 80, 24).unwrap()
}

fn text(app: &mut App) -> String {
    app.current_tab().rendered_content.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")
}

/// Handle messages until one matching `done` has been handled
async fn settle(app: &mut App, done: fn(&NetworkResponse) -> bool) {
    loop {
        let resp = tokio::time::timeout(std::time::Duration::from_secs(10), app.rx.recv()).await.unwrap().unwrap();
        let finished = done(&resp);
        handle_network_event(app, resp, 80).unwrap();
        if finished {
            return;
        }
    }
}

fn set_field(app: &mut App, name: &str, value: &str) {
    let tab = app.current_tab();
    let index = tab.controls.iter().position(|c| c.name == name).unwrap();
    tab.control_values[index] = ControlValue::Text(value.to_string());
}

#[tokio::test]
async fn a_sites_tools_are_listed_confirmed_and_run() {
    for stream in [false, true] {
        let (server, received) = shop(stream).await;
        let mut app = test_app();
        app.current_tab().url_input = format!("{}/", server.uri());
        app.submit_request();
        settle(&mut app, |r| matches!(r, NetworkResponse::Success(..))).await;
        assert!(text(&mut app).starts_with("Tools: Open this site's tools"), "{}", text(&mut app));

        // Follow the Tools link
        press(&mut app, KeyCode::Enter);
        let tools_page = mcp::tools_address(&format!("{}/mcp", server.uri()));
        assert_eq!(app.current_tab().url_input, tools_page);
        settle(&mut app, |r| matches!(r, NetworkResponse::ToolsListed(..))).await;
        let page = text(&mut app);
        assert!(page.contains("Search the shop") && page.contains("Finds <products>."), "{page}");
        assert!(page.contains("Words to look for"), "{page}");
        assert!(page.contains("may change or delete things"), "unannotated tools are flagged: {page}");
        assert_eq!(methods(&received), ["initialize", "notifications/initialized", "tools/list"]);

        // A required field left empty
        app.submit_form(0, None);
        assert!(app.current_tab().status_message.contains("Fill in \"query\""));
        assert!(app.current_tab().tool_prompt.is_none());

        set_field(&mut app, "query", "red shoes");
        app.submit_form(0, None);
        let prompt = app.current_tab().tool_prompt.as_ref().expect("asks first");
        assert_eq!(prompt.arguments, json!({ "query": "red shoes", "limit": 10 }));
        assert!(prompt.warning.is_none(), "the search is read-only");
        let mut terminal = ratatui::Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| rynx_browser::ui::ui(f, &app)).unwrap();
        let screen: String = terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(screen.contains("Run \"Search the shop\" on 127.0.0.1?"), "{screen}");
        assert!(screen.contains("\"query\": \"red shoes\""), "{screen}");
        assert!(screen.contains("[Y] Run"), "{screen}");

        // While asking, other keys do nothing (not even quitting); n cancels
        assert!(!press(&mut app, KeyCode::Char('q')));
        assert!(app.current_tab().tool_prompt.is_some());
        press(&mut app, KeyCode::Char('n'));
        assert!(app.current_tab().tool_prompt.is_none());
        assert_eq!(methods(&received).len(), 3, "nothing sent before confirming");

        app.submit_form(0, None);
        press(&mut app, KeyCode::Char('y'));
        settle(&mut app, |r| matches!(r, NetworkResponse::ToolFinished(..))).await;
        assert_eq!(methods(&received).last().unwrap(), "tools/call");
        assert!(app.current_tab().url_input.starts_with(mcp::RESULT_PAGE));
        assert!(text(&mut app).contains("Found 3 results for \"red shoes\""), "{}", text(&mut app));

        // Back to the tools, then forward to the kept result without running it again
        press(&mut app, KeyCode::Backspace);
        settle(&mut app, |r| matches!(r, NetworkResponse::ToolsListed(..))).await;
        assert_eq!(app.current_tab().url_input, tools_page);
        press(&mut app, KeyCode::Right);
        assert!(text(&mut app).contains("Found 3 results"));
        assert_eq!(methods(&received).iter().filter(|m| *m == "tools/call").count(), 1);
        assert_eq!(methods(&received).iter().filter(|m| *m == "initialize").count(), 1, "the session is kept");
    }
}

#[tokio::test]
async fn opening_a_server_as_a_page_offers_its_tools() {
    let (server, _) = shop(false).await;
    let mut app = test_app();
    app.current_tab().url_input = format!("{}/mcp", server.uri());
    app.submit_request();
    settle(&mut app, |r| matches!(r, NetworkResponse::Success(..))).await;
    let page = text(&mut app);
    assert!(page.contains("is an MCP server") && !page.contains("Not Acceptable"), "{page}");

    press(&mut app, KeyCode::Enter);
    settle(&mut app, |r| matches!(r, NetworkResponse::ToolsListed(..))).await;
    assert!(text(&mut app).contains("Search the shop"));

    // Ordinary JSON stays JSON
    assert!(!mcp::is_endpoint_answer(Some("application/json"), r#"{"name": "x"}"#));
    assert!(!mcp::is_endpoint_answer(Some("text/plain"), r#"{"jsonrpc": "2.0"}"#));
}

#[tokio::test]
async fn web_pages_cannot_reach_tools_directly() {
    let mut app = test_app();
    let id = app.current_tab().id;
    let html = "<form action=\"about:tool-call?tool=delete_cart\"><input type=submit value=Go></form>\
                <a href=\"about:tool-result?id=1\">old result</a>";
    app.current_tab().url_input = "https://evil.example/".into();
    let resp = NetworkResponse::Success(id, "t".into(), html.into(), "https://evil.example/".into(), PageKind::Html);
    handle_network_event(&mut app, resp, 80).unwrap();
    app.submit_form(0, None);
    assert!(app.current_tab().tool_prompt.is_none());
    assert!(app.current_tab().status_message.contains("Not sent"), "{}", app.current_tab().status_message);

    app.follow_link("about:tool-result?id=1".into());
    assert_eq!(app.current_tab().url_input, "https://evil.example/");

    // A tools link to another site is refused
    app.follow_link(mcp::tools_address("https://bank.example/mcp"));
    assert!(app.current_tab().status_message.contains("another site"));
    assert_eq!(app.current_tab().url_input, "https://evil.example/");
}

#[tokio::test]
async fn servers_that_redirect_or_fail_are_explained() {
    let server = MockServer::start().await;
    Mock::given(path("/moved"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "http://169.254.169.254/"))
        .mount(&server)
        .await;
    Mock::given(path("/login")).respond_with(ResponseTemplate::new(401)).mount(&server).await;
    Mock::given(path("/old"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "jsonrpc": "2.0", "id": 1, "result": { "protocolVersion": "2024-11-05" } })))
        .mount(&server)
        .await;

    for (endpoint, expected) in [("moved", "can't redirect"), ("login", "sign in"), ("old", "version")] {
        let mut app = test_app();
        app.current_tab().url_input = mcp::tools_address(&format!("{}/{}", server.uri(), endpoint));
        app.load_url_input(false);
        settle(&mut app, |r| matches!(r, NetworkResponse::ToolsListed(..))).await;
        assert!(text(&mut app).contains(expected), "{endpoint}: {}", text(&mut app));
    }
}

#[tokio::test]
#[ignore = "needs the internet"]
async fn live_mcp_server() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let http = reqwest::Client::new();
    let client = mcp::Client::default();
    let server = "https://mcp.deepwiki.com/mcp";
    let tools = client.list_tools(&http, server).await.unwrap();
    println!("{}", tools.iter().map(|t| format!("{} ({})", t.title, t.name)).collect::<Vec<_>>().join(", "));
    let tool = tools.iter().find(|t| t.name == "read_wiki_structure").expect("a known tool");
    let args = mcp::arguments(tool, &[("repoName".to_string(), mcp::FieldValue::Text("rust-lang/rust".into()))].into()).unwrap();
    let result = client.call_tool(&http, server, &tool.name, args).await.unwrap();
    let html = mcp::result_page_html(server, &tool.title, &Ok(result));
    println!("{}", &html[..html.len().min(600)]);
    assert!(html.contains("<p>"));
}

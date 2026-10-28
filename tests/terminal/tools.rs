//! Site tools (MCP), against a local MCP server (MCP in TESTING.md)

use super::harness::*;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

/// Answers MCP requests as a small shop's server would, recording the tool calls
struct Shop {
    calls: Arc<Mutex<Vec<Value>>>,
}

impl wiremock::Respond for Shop {
    fn respond(&self, request: &wiremock::Request) -> wiremock::ResponseTemplate {
        let message: Value = serde_json::from_slice(&request.body).unwrap();
        let id = message["id"].clone();
        let answer = |result: Value| json!({ "jsonrpc": "2.0", "id": id, "result": result });
        let ok = |body: Value| {
            wiremock::ResponseTemplate::new(200).insert_header("mcp-session-id", "s1").set_body_json(body)
        };
        match message["method"].as_str().unwrap_or("") {
            "initialize" => ok(answer(
                json!({ "protocolVersion": "2025-06-18", "capabilities": { "tools": {} }, "serverInfo": { "name": "shop" } }),
            )),
            "notifications/initialized" => wiremock::ResponseTemplate::new(202),
            "tools/list" => ok(answer(json!({ "tools": [
                { "name": "search", "title": "Search the shop", "description": "Finds products.",
                  "annotations": { "readOnlyHint": true },
                  "inputSchema": { "type": "object", "properties": { "query": { "type": "string", "description": "Words to look for" } }, "required": ["query"] } },
                { "name": "delete_cart", "title": "Empty the cart", "description": "Empties your cart", "inputSchema": { "type": "object" } }
            ]}))),
            "tools/call" => {
                self.calls.lock().unwrap().push(message["params"].clone());
                let query = message["params"]["arguments"]["query"].as_str().unwrap_or("").to_string();
                ok(answer(json!({ "content": [{ "type": "text", "text": format!("Found 3 results for {query}") }] })))
            }
            _ => ok(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "no such method" } })),
        }
    }
}

fn shop() -> (Site, Arc<Mutex<Vec<Value>>>) {
    let site = Site::new(&[]);
    let calls = Arc::new(Mutex::new(Vec::new()));
    site.mount(
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/mcp"))
            .respond_with(Shop { calls: Arc::clone(&calls) }),
    );
    // Opened as a page, the server refuses the way real ones do
    site.respond(
        "/mcp",
        wiremock::ResponseTemplate::new(406).set_body_json(json!({ "jsonrpc": "2.0", "id": "x",
            "error": { "code": -32600, "message": "Not Acceptable: Client must accept text/event-stream" } })),
    );
    (site, calls)
}

fn tools_page(name: &str) -> (Home, Rynx, Site, Arc<Mutex<Vec<Value>>>) {
    let (site, calls) = shop();
    let home = Home::new(name);
    home.settings("");
    let mut rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    rynx.open(&site.url("/mcp"));
    rynx.wait_for("Open its tools");
    rynx.click_text("Open its tools");
    rynx.wait_for("Tools from 127.0.0.1");
    (home, rynx, site, calls)
}

/// Type into the query field (the first on the page) and press Enter
fn search(rynx: &mut Rynx, words: &str) {
    let (row, _) = rynx.find("query").expect("the query field");
    let (field_row, col) =
        (row..row + 4).find_map(|r| rynx.rows()[r as usize].find("[__").map(|c| (r, c as u16))).expect("its box");
    rynx.click(field_row, col + 2);
    rynx.wait_for("Typing");
    rynx.keys("\u{15}");
    rynx.type_text(words);
    rynx.keys("\r");
}

/// MCP-01: an MCP server opened as a page offers its tools, listed with their fields
#[test]
fn mcp_01_listing_tools() {
    let (_home, mut rynx, _site, _calls) = tools_page("mcp-list");
    rynx.wait_for("Search the shop");
    rynx.wait_for("Finds products.");
    rynx.wait_for("(required)");
    rynx.wait_for("Words to look for");
    rynx.wait_for("[ Run Search the shop ]");
    rynx.wait_for("Empty the cart");
    rynx.quit();
}

/// MCP-02, MCP-03: a call is shown before it's sent; n sends nothing, y runs it; Back and
/// Forward don't run it again
#[test]
fn mcp_02_03_confirming_a_call() {
    let (_home, mut rynx, _site, calls) = tools_page("mcp-call");
    search(&mut rynx, "red shoes");
    rynx.wait_for("Run a tool");
    rynx.wait_for("\"query\": \"red shoes\"");
    // Other keys and clicks do nothing while it asks
    rynx.keys("q");
    rynx.click(10, 10);
    assert!(rynx.is_running());
    rynx.wait_for("Run a tool");
    rynx.keys("n");
    rynx.wait_for("Cancelled: nothing was sent");
    assert!(calls.lock().unwrap().is_empty(), "something was sent");
    rynx.click_text("[ Run Search the shop ]");
    rynx.wait_for("Run a tool");
    rynx.keys("y");
    rynx.wait_for("Found 3 results for red shoes");
    assert_eq!(calls.lock().unwrap().len(), 1);
    rynx.keys("\u{7f}");
    rynx.wait_for("Tools from 127.0.0.1");
    rynx.keys("L");
    rynx.wait_for("Found 3 results for red shoes");
    assert_eq!(calls.lock().unwrap().len(), 1, "going back and forward ran the tool again");
    rynx.quit();
}

/// A tool not marked read-only warns that it may change things
#[test]
fn mcp_02_a_tool_that_may_change_things_says_so() {
    let (_home, mut rynx, _site, _calls) = tools_page("mcp-warning");
    rynx.scroll_to("[ Run Empty the cart ]");
    rynx.click_text("[ Run Empty the cart ]");
    rynx.wait_for("Run a tool");
    rynx.wait_for("may change");
    rynx.keys("n");
    rynx.quit();
}

/// MCP-04: a required field left empty is named, and nothing is asked or sent
#[test]
fn mcp_04_validation() {
    let (_home, mut rynx, _site, calls) = tools_page("mcp-required");
    rynx.click_text("[ Run Search the shop ]");
    rynx.wait_for("Fill in \"query\" first");
    assert!(!rynx.text().contains("Run a tool"));
    assert!(calls.lock().unwrap().is_empty());
    rynx.quit();
}

/// MCP-05: the download box keeps every key but y and n
#[test]
fn mcp_05_download_box_keeps_its_keys() {
    let site = Site::new(&[("/", "<p><a href=\"/f.zip\">A file</a></p>")]);
    site.respond("/f.zip", wiremock::ResponseTemplate::new(200).set_body_raw(b"PK".to_vec(), "application/zip"));
    let home = Home::new("download-box");
    home.settings("");
    let mut rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    rynx.open(&site.url("/"));
    rynx.wait_for("A file");
    rynx.keys("d");
    rynx.wait_for("f.zip");
    rynx.keys("q");
    rynx.keys("j");
    assert!(rynx.is_running(), "q quit with the box open");
    rynx.wait_for("f.zip");
    rynx.keys("n");
    rynx.wait_gone("f.zip?");
    rynx.quit();
}

//! MCP (Model Context Protocol) tools: an alternative to JavaScript for interactive sites.
//!
//! A page announces an MCP server with `<link rel="mcp" href="/mcp">`. The browser lists the
//! server's tools as forms on its own page (about:tools), and calls a tool only after the user has
//! seen exactly what will be sent and confirmed it. Results are shown as a page.
//!
//! Only the client side of MCP's Streamable HTTP transport that tools need is implemented:
//! initialize, tools/list and tools/call, over the browser's own HTTP client so HTTPS upgrades,
//! size limits and timeouts apply. Servers can't redirect, ask the user questions, or ask for AI
//! sampling. No AI model is involved, so tool descriptions are just text shown to the user.

use crate::network::escape_html;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use url::Url;

/// The protocol version asked for; servers may answer with any of SUPPORTED_VERSIONS
const PROTOCOL_VERSION: &str = "2025-06-18";
const SUPPORTED_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];

/// The page listing a server's tools: about:tools?server=<address>
pub const TOOLS_PAGE: &str = "about:tools?server=";
/// A tool's result: about:tool-result?id=<n>
pub const RESULT_PAGE: &str = "about:tool-result?id=";
/// Where the tools page's forms send their data; handled by the browser, never fetched
pub const CALL_ACTION: &str = "about:tool-call";

/// Most tools listed from one server, and most input fields shown for one tool
const MAX_TOOLS: usize = 200;
const MAX_FIELDS: usize = 50;

/// The tools page for `server`
pub fn tools_address(server: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(server.as_bytes()).collect();
    format!("{}{}", TOOLS_PAGE, encoded)
}

/// The server named in a tools page address
pub fn server_of(address: &str) -> Option<String> {
    let query = address.strip_prefix("about:tools?")?;
    url::form_urlencoded::parse(query.as_bytes()).find(|(k, _)| k == "server").map(|(_, v)| v.into_owned())
}

/// The tool a form on the tools page runs
pub fn tool_of_action(action: &str) -> Option<String> {
    let query = action.strip_prefix(CALL_ACTION)?.strip_prefix('?')?;
    url::form_urlencoded::parse(query.as_bytes()).find(|(k, _)| k == "tool").map(|(_, v)| v.into_owned())
}

fn host_of(url: &Url) -> String {
    url.host_str().unwrap_or("").trim_start_matches("www.").to_ascii_lowercase()
}

/// Check and resolve a server address. `page` is the page that announced or linked to it, if any;
/// without one the user typed it, and only the basic checks apply.
pub fn check_server(server: &str, page: Option<&str>, i2p_mode: bool) -> Result<String, String> {
    let resolved = match page {
        Some(page) => Url::parse(page).and_then(|p| p.join(server)),
        None => Url::parse(server),
    }
    .map_err(|_| format!("Invalid tools address: {}", server))?;
    let resolved = Url::parse(&crate::network::enforce_https(resolved.as_str(), i2p_mode))
        .map_err(|_| format!("Invalid tools address: {}", server))?;
    if !matches!(resolved.scheme(), "http" | "https") {
        return Err(format!("Tools must be on a web address, not {}:", resolved.scheme()));
    }
    if !resolved.username().is_empty() || resolved.password().is_some() {
        return Err("Blocked: the tools address hides where it really goes".to_string());
    }
    if let Some(page) = page.and_then(|p| Url::parse(p).ok()) {
        // A page may only offer its own site's tools: anything else would be a disguise
        let (site, server_host) = (host_of(&page), host_of(&resolved));
        if server_host != site && !server_host.ends_with(&format!(".{}", site)) {
            return Err(format!("Blocked: {} offered tools from another site ({})", site, server_host));
        }
        // Local MCP servers (developer tools and the like) often trust anything that reaches them
        if crate::network::is_local_network_url(resolved.as_str())
            && !crate::network::is_local_network_url(page.as_str())
        {
            return Err("Blocked: a public page can't use tools on your local network".to_string());
        }
    }
    Ok(resolved.to_string())
}

/// A tool a server offers
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub name: String,
    pub title: String,
    pub description: String,
    pub input_schema: Value,
    /// The server says the tool only reads information
    pub read_only: bool,
    /// The server says the tool may delete or overwrite things
    pub destructive: bool,
}

impl Tool {
    fn from_json(value: &Value) -> Option<Tool> {
        let name = value.get("name")?.as_str()?.to_string();
        let annotations = value.get("annotations");
        let title = value
            .get("title")
            .or_else(|| annotations.and_then(|a| a.get("title")))
            .and_then(Value::as_str)
            .filter(|t| !t.trim().is_empty())
            .unwrap_or(&name)
            .to_string();
        let hint = |key| annotations.and_then(|a| a.get(key)).and_then(Value::as_bool);
        let read_only = hint("readOnlyHint").unwrap_or(false);
        Some(Tool {
            name,
            title,
            description: value.get("description").and_then(Value::as_str).unwrap_or("").to_string(),
            input_schema: value.get("inputSchema").cloned().unwrap_or(Value::Null),
            read_only,
            // The spec's default: a tool that isn't read-only may be destructive
            destructive: !read_only && hint("destructiveHint").unwrap_or(true),
        })
    }

    /// The tool's input fields, in the order the schema lists them (serde_json keeps that order
    /// only with preserve_order, so required fields come first, then the rest by name)
    fn fields(&self) -> Vec<(String, Value, bool)> {
        let Some(properties) = self.input_schema.get("properties").and_then(Value::as_object) else {
            return Vec::new();
        };
        let required: Vec<&str> = self
            .input_schema
            .get("required")
            .and_then(Value::as_array)
            .map(|r| r.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let mut fields: Vec<(String, Value, bool)> = properties
            .iter()
            .map(|(name, schema)| (name.clone(), schema.clone(), required.contains(&name.as_str())))
            .collect();
        fields.sort_by_key(|(name, _, required)| (!required, name.clone()));
        fields.truncate(MAX_FIELDS);
        fields
    }

    /// What the confirmation prompt warns about
    pub fn warning(&self) -> Option<&'static str> {
        if self.read_only {
            None
        } else if self.destructive {
            Some("This tool may change or delete things on the site.")
        } else {
            Some("This tool may change things on the site.")
        }
    }
}

/// The JSON type a schema asks for
fn schema_type(schema: &Value) -> &str {
    match schema.get("type") {
        Some(Value::String(t)) => t,
        // ["string", "null"] and the like: the first non-null type
        Some(Value::Array(types)) => types.iter().filter_map(Value::as_str).find(|t| *t != "null").unwrap_or("string"),
        _ if schema.get("enum").is_some() => "string",
        _ => "",
    }
}

fn enum_values(schema: &Value) -> Option<Vec<String>> {
    let values = schema.get("enum")?.as_array()?;
    Some(values.iter().map(as_text).collect())
}

/// A JSON value as the text shown in a form: strings without their quotes
fn as_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

// --- Pages ---

/// The tools page: each tool with its description and a form for its input
pub fn tools_page_html(server: &str, tools: &[Tool]) -> String {
    let host = Url::parse(server).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
    let mut html = format!(
        "<title>Tools from {host}</title><h1>Tools from {host}</h1>\
         <p>These tools are run by {host} at {server}. Nothing is sent until you run one and confirm.</p>",
        host = escape_html(&host),
        server = escape_html(server),
    );
    if tools.is_empty() {
        html.push_str("<p>This server offers no tools.</p>");
    }
    for tool in tools {
        html.push_str(&format!("<hr><h2>{}</h2>", escape_html(&tool.title)));
        for paragraph in tool.description.split("\n\n").filter(|p| !p.trim().is_empty()) {
            html.push_str(&format!("<p>{}</p>", escape_html(paragraph.trim()).replace('\n', "<br>")));
        }
        if let Some(warning) = tool.warning() {
            html.push_str(&format!("<p><i>{}</i></p>", warning));
        }
        let action: String =
            url::form_urlencoded::Serializer::new(String::new()).append_pair("tool", &tool.name).finish();
        html.push_str(&format!("<form action=\"{}?{}\">", CALL_ACTION, escape_html(&action)));
        for (name, schema, required) in tool.fields() {
            let label = schema.get("title").and_then(Value::as_str).unwrap_or(&name);
            let mut text = format!("<b>{}</b>", escape_html(label));
            if required {
                text.push_str(" (required)");
            }
            if let Some(description) = schema.get("description").and_then(Value::as_str) {
                text.push_str(&format!(": {}", escape_html(description)));
            }
            html.push_str(&format!("<div>{}</div><div>{}</div>", text, field_html(&name, &schema, required)));
        }
        html.push_str(&format!("<div><input type=\"submit\" value=\"Run {}\"></div></form>", escape_html(&tool.title)));
    }
    html
}

/// The form control for one input field
fn field_html(name: &str, schema: &Value, required: bool) -> String {
    let name = escape_html(name);
    let default = schema.get("default");
    if let Some(values) = enum_values(schema) {
        let mut html = format!("<select name=\"{}\">", name);
        if !required {
            html.push_str("<option value=\"\">(none)</option>");
        }
        for value in values {
            let selected = default.map(as_text).as_ref() == Some(&value);
            html.push_str(&format!(
                "<option value=\"{0}\"{1}>{0}</option>",
                escape_html(&value),
                if selected { " selected" } else { "" }
            ));
        }
        return html + "</select>";
    }
    match schema_type(schema) {
        "boolean" => format!(
            "<input type=\"checkbox\" name=\"{}\" value=\"true\"{}>",
            name,
            if default == Some(&Value::Bool(true)) { " checked" } else { "" }
        ),
        "string" | "integer" | "number" | "" => {
            let value = match default {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Number(n)) => n.to_string(),
                _ => String::new(),
            };
            format!("<input type=\"text\" name=\"{}\" size=\"50\" value=\"{}\">", name, escape_html(&value))
        }
        // Arrays and objects are typed as JSON
        _ => {
            let value = default.map(|d| serde_json::to_string_pretty(d).unwrap_or_default()).unwrap_or_default();
            format!(
                "<textarea name=\"{}\" rows=\"4\" cols=\"60\" placeholder=\"JSON\">{}</textarea>",
                name,
                escape_html(&value)
            )
        }
    }
}

/// What the user entered into one field of a tool's form
#[derive(Clone, Debug, PartialEq)]
pub enum FieldValue {
    Text(String),
    Checked(bool),
}

/// Turn the form's fields into the tool's arguments, typed as its schema asks
pub fn arguments(tool: &Tool, entered: &HashMap<String, FieldValue>) -> Result<Value, String> {
    let mut arguments = Map::new();
    for (name, schema, required) in tool.fields() {
        let value = match entered.get(&name) {
            Some(FieldValue::Checked(on)) => Some(Value::Bool(*on)),
            Some(FieldValue::Text(text)) if text.trim().is_empty() => None,
            Some(FieldValue::Text(text)) => Some(typed_value(&name, &schema, text)?),
            None => None,
        };
        match value {
            Some(value) => {
                arguments.insert(name, value);
            }
            None if required => return Err(format!("Fill in \"{}\" first", name)),
            None => {}
        }
    }
    Ok(Value::Object(arguments))
}

fn typed_value(name: &str, schema: &Value, text: &str) -> Result<Value, String> {
    let trimmed = text.trim();
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        // Give back the server's own value, which may be a number
        return values
            .iter()
            .find(|v| as_text(v) == text)
            .cloned()
            .ok_or_else(|| format!("\"{}\" must be one of the listed choices", name));
    }
    match schema_type(schema) {
        "integer" => {
            trimmed.parse::<i64>().map(Value::from).map_err(|_| format!("\"{}\" must be a whole number", name))
        }
        "number" => trimmed
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| format!("\"{}\" must be a number", name)),
        "array" | "object" => {
            let value: Value =
                serde_json::from_str(trimmed).map_err(|e| format!("\"{}\" must be JSON: {}", name, e))?;
            let matches = if schema_type(schema) == "array" { value.is_array() } else { value.is_object() };
            if matches { Ok(value) } else { Err(format!("\"{}\" must be a JSON {}", name, schema_type(schema))) }
        }
        _ => Ok(Value::String(text.to_string())),
    }
}

/// A tool's result as a page
pub fn result_page_html(server: &str, tool: &str, result: &Result<Value, String>) -> String {
    let mut html = format!("<title>{0}</title><h1>{0}</h1>", escape_html(tool));
    let result = match result {
        Ok(result) => result,
        Err(e) => {
            html.push_str(&format!("<p>The tool couldn't be run: {}</p>", escape_html(e)));
            return html + &back_link(server);
        }
    };
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        html.push_str("<p><b>The tool reported an error:</b></p>");
    }
    let content = result.get("content").and_then(Value::as_array).cloned().unwrap_or_default();
    for item in &content {
        match item.get("type").and_then(Value::as_str) {
            Some("text") => {
                let text = item.get("text").and_then(Value::as_str).unwrap_or("");
                for paragraph in text.split("\n\n").filter(|p| !p.trim().is_empty()) {
                    let lines: Vec<String> = paragraph.trim_end().lines().map(keep_indent).collect();
                    html.push_str(&format!("<p>{}</p>", lines.join("<br>")));
                }
            }
            Some("resource_link") => {
                let uri = item.get("uri").and_then(Value::as_str).unwrap_or("");
                let name = item.get("title").or_else(|| item.get("name")).and_then(Value::as_str).unwrap_or(uri);
                html.push_str(&format!("<p>Link: <a href=\"{}\">{}</a></p>", escape_html(uri), escape_html(name)));
            }
            Some("resource") => match item.get("resource").and_then(|r| r.get("text")).and_then(Value::as_str) {
                Some(text) => html.push_str(&format!("<pre>{}</pre>", escape_html(text))),
                None => html.push_str("<p>[A file, which can't be shown]</p>"),
            },
            Some(kind @ ("image" | "audio")) => {
                let mime = item.get("mimeType").and_then(Value::as_str).unwrap_or(kind);
                html.push_str(&format!("<p>[{} ({}), which can't be shown yet]</p>", kind, escape_html(mime)));
            }
            _ => {}
        }
    }
    if content.is_empty() {
        match result.get("structuredContent") {
            Some(data) => html.push_str(&format!(
                "<pre>{}</pre>",
                escape_html(&serde_json::to_string_pretty(data).unwrap_or_default())
            )),
            None => html.push_str("<p>The tool finished without a result.</p>"),
        }
    }
    html + &back_link(server)
}

/// A line of text as HTML, keeping its indentation (nested lists in tool output rely on it)
fn keep_indent(line: &str) -> String {
    let text = line.trim_start_matches(' ');
    "&nbsp;".repeat(line.len() - text.len()) + &escape_html(text)
}

fn back_link(server: &str) -> String {
    format!("<hr><p><a href=\"{}\">Back to the tools</a></p>", escape_html(&tools_address(server)))
}

/// Whether a response to an ordinary page request is a JSON-RPC message: what an MCP server
/// answers when its address is opened as a page
pub fn is_endpoint_answer(content_type: Option<&str>, body: &str) -> bool {
    let json = content_type
        .is_some_and(|ct| ct.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("application/json"));
    json && body.len() < 64 * 1024
        && serde_json::from_str::<Value>(body).is_ok_and(|v| v.get("jsonrpc").and_then(Value::as_str) == Some("2.0"))
}

/// The page shown when an MCP server's address is opened as a page
pub fn endpoint_page_html(url: &str) -> String {
    format!(
        "<title>MCP server</title><h1>MCP server</h1><p>{} is an MCP server, which offers tools \
         rather than a web page.</p><p><a href=\"{}\">Open its tools</a></p>",
        escape_html(url),
        escape_html(&tools_address(url))
    )
}

/// A page explaining why the tools couldn't be listed
pub fn error_page_html(server: &str, error: &str) -> String {
    format!(
        "<title>Tools unavailable</title><h1>Tools unavailable</h1><p>Couldn't get the tools from {}:</p><p>{}</p>",
        escape_html(server),
        escape_html(error)
    )
}

// --- The protocol ---

/// A connection to a server, kept between requests
#[derive(Clone, Debug)]
struct Session {
    id: Option<String>,
    version: String,
}

/// Sessions with the servers used so far
#[derive(Debug, Default)]
pub struct Client {
    sessions: tokio::sync::Mutex<HashMap<String, Session>>,
    next_id: std::sync::atomic::AtomicU64,
}

enum RpcError {
    /// The server forgot the session; start a new one
    SessionExpired,
    Failed(String),
}

impl From<String> for RpcError {
    fn from(message: String) -> Self {
        RpcError::Failed(message)
    }
}

impl Client {
    /// List a server's tools
    pub async fn list_tools(&self, http: &reqwest::Client, server: &str) -> Result<Vec<Tool>, String> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        // Servers return tools a page at a time
        for _ in 0..10 {
            let params = match &cursor {
                Some(cursor) => json!({ "cursor": cursor }),
                None => json!({}),
            };
            let result = self.request(http, server, "tools/list", params).await?;
            let listed = result.get("tools").and_then(Value::as_array).ok_or("The server sent no tool list")?;
            tools.extend(listed.iter().filter_map(Tool::from_json));
            cursor = result.get("nextCursor").and_then(Value::as_str).map(str::to_string);
            if cursor.is_none() || tools.len() >= MAX_TOOLS {
                break;
            }
        }
        tools.truncate(MAX_TOOLS);
        Ok(tools)
    }

    /// Run a tool
    pub async fn call_tool(
        &self,
        http: &reqwest::Client,
        server: &str,
        tool: &str,
        arguments: Value,
    ) -> Result<Value, String> {
        self.request(http, server, "tools/call", json!({ "name": tool, "arguments": arguments })).await
    }

    /// Send a request, starting a session first if needed, and once more if the session expired
    async fn request(
        &self,
        http: &reqwest::Client,
        server: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        for _ in 0..2 {
            let session = self.session(http, server).await?;
            match self.rpc(http, server, Some(&session), method, params.clone()).await {
                Ok((result, _)) => return result.ok_or_else(|| "The server sent no answer".to_string()),
                Err(RpcError::SessionExpired) => {
                    self.sessions.lock().await.remove(server);
                }
                Err(RpcError::Failed(e)) => return Err(e),
            }
        }
        Err("The server keeps ending the session".to_string())
    }

    async fn session(&self, http: &reqwest::Client, server: &str) -> Result<Session, String> {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.get(server) {
            return Ok(session.clone());
        }
        let params = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "rynx-browser", "version": env!("CARGO_PKG_VERSION") },
        });
        let (result, id) = match self.rpc(http, server, None, "initialize", params).await {
            Ok(answer) => answer,
            Err(RpcError::SessionExpired) => return Err("The server refused to start a session".to_string()),
            Err(RpcError::Failed(e)) => return Err(e),
        };
        let result = result.ok_or("The server didn't answer the greeting")?;
        let version = result.get("protocolVersion").and_then(Value::as_str).unwrap_or("").to_string();
        if !SUPPORTED_VERSIONS.contains(&version.as_str()) {
            return Err(format!("The server speaks an MCP version rynx doesn't know ({})", version));
        }
        let session = Session { id, version };
        // Tell the server we're ready; it answers with nothing
        self.notify(http, server, &session, "notifications/initialized").await?;
        sessions.insert(server.to_string(), session.clone());
        Ok(session)
    }

    async fn notify(
        &self,
        http: &reqwest::Client,
        server: &str,
        session: &Session,
        method: &str,
    ) -> Result<(), String> {
        let body = json!({ "jsonrpc": "2.0", "method": method });
        let response = post(http, server, Some(session), &body).await?;
        if !response.status().is_success() {
            return Err(format!("The server refused {} ({})", method, response.status()));
        }
        Ok(())
    }

    /// One JSON-RPC request: its result, and the session id the server gave, if any
    async fn rpc(
        &self,
        http: &reqwest::Client,
        server: &str,
        session: Option<&Session>,
        method: &str,
        params: Value,
    ) -> Result<(Option<Value>, Option<String>), RpcError> {
        let id = self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let body = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let response = post(http, server, session, &body).await?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND && session.and_then(|s| s.id.as_ref()).is_some() {
            return Err(RpcError::SessionExpired);
        }
        if status.is_redirection() {
            return Err(RpcError::Failed("MCP servers can't redirect".to_string()));
        }
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(RpcError::Failed(
                "This server needs you to sign in, which rynx doesn't support yet".to_string(),
            ));
        }
        if !status.is_success() {
            return Err(RpcError::Failed(format!("The server answered {}", status)));
        }
        let session_id = response
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            // Sent back in a header, so it must be visible ASCII
            .filter(|v| v.bytes().all(|b| (0x21..=0x7e).contains(&b)))
            .map(str::to_string);
        let streamed = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.starts_with("text/event-stream"));
        let text = crate::network::read_text_limited(response, crate::constants::MAX_PAGE_SIZE_BYTES).await?;
        let messages: Vec<Value> = if streamed {
            server_sent_events(&text).iter().filter_map(|data| serde_json::from_str(data).ok()).collect()
        } else {
            let value: Value =
                serde_json::from_str(&text).map_err(|_| "The server's answer isn't valid JSON".to_string())?;
            // A batch is an array of messages
            match value {
                Value::Array(messages) => messages,
                message => vec![message],
            }
        };
        let answer = messages
            .into_iter()
            .find(|m| m.get("id").and_then(Value::as_u64) == Some(id))
            .ok_or_else(|| "The server didn't answer the request".to_string())?;
        if let Some(error) = answer.get("error") {
            let message = error.get("message").and_then(Value::as_str).unwrap_or("unknown error");
            return Err(RpcError::Failed(format!("The server reported an error: {}", message)));
        }
        Ok((answer.get("result").cloned(), session_id))
    }
}

async fn post(
    http: &reqwest::Client,
    server: &str,
    session: Option<&Session>,
    body: &Value,
) -> Result<reqwest::Response, String> {
    let mut request = http
        .post(server)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::ACCEPT, "application/json, text/event-stream")
        .body(body.to_string());
    if let Some(session) = session {
        request = request.header("MCP-Protocol-Version", &session.version);
        if let Some(id) = &session.id {
            request = request.header("Mcp-Session-Id", id);
        }
    }
    request.send().await.map_err(|e| format!("Couldn't reach the tools server: {}", e))
}

/// The data of each event in a text/event-stream body
fn server_sent_events(text: &str) -> Vec<String> {
    let mut events = Vec::new();
    let mut data: Option<String> = None;
    for line in text.lines() {
        if line.is_empty() {
            events.extend(data.take());
        } else if let Some(value) = line.strip_prefix("data:") {
            let value = value.strip_prefix(' ').unwrap_or(value);
            match &mut data {
                Some(data) => {
                    data.push('\n');
                    data.push_str(value);
                }
                None => data = Some(value.to_string()),
            }
        }
    }
    events.extend(data);
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_split() {
        let events = server_sent_events("event: message\ndata: {\"a\":\ndata: 1}\n\n: comment\ndata:{\"b\":2}\n");
        assert_eq!(events, ["{\"a\":\n1}", "{\"b\":2}"]);
    }

    #[test]
    fn tools_addresses_round_trip() {
        let address = tools_address("https://x.example/mcp?a=1&b=2");
        assert_eq!(server_of(&address).as_deref(), Some("https://x.example/mcp?a=1&b=2"));
        assert_eq!(tool_of_action("about:tool-call?tool=a+b%26c").as_deref(), Some("a b&c"));
    }
}

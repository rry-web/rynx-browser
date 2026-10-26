//! Site tools over MCP: listing a server's tools as a page, confirming a call, showing results.

use super::*;

impl App {
    /// The address of a tools link followed from the current page, after checking the page may
    /// offer that server's tools. None (with the reason in the status line) if it may not.
    pub(super) fn checked_tools_link(&mut self, url: &str) -> Option<String> {
        let server = crate::mcp::server_of(url)?;
        let page = self.tabs[self.active_tab_index].page_url.clone();
        match crate::mcp::check_server(&server, page.as_deref(), self.i2p_mode) {
            Ok(server) => Some(crate::mcp::tools_address(&server)),
            Err(e) => {
                self.current_tab().status_message = e;
                None
            }
        }
    }

    /// Show the tools page for `server`, listing its tools in the background
    pub(super) fn open_tools(&mut self, server: &str) {
        let index = self.active_tab_index;
        let server = match crate::mcp::check_server(server, None, self.i2p_mode) {
            Ok(server) => server,
            Err(e) => {
                let address = crate::mcp::tools_address(server);
                self.show_generated_page(index, &address, "Tools unavailable", crate::mcp::error_page_html(server, &e));
                return;
            }
        };
        let address = crate::mcp::tools_address(&server);
        let tab = self.current_tab();
        tab.url_input = address.clone();
        tab.page_title = String::from("Loading...");
        tab.status_message = String::from("Getting the site's tools...");
        let id = tab.id;
        let tx = self.tx.clone();
        let http = self.network_manager.get_client(self.i2p_mode).clone();
        let mcp = Arc::clone(&self.mcp);
        tokio::spawn(async move {
            let result = mcp.list_tools(&http, &server).await;
            let _ = tx.send(NetworkResponse::ToolsListed(id, address, server, result)).await;
        });
    }

    /// Show the tools a server listed, if the tab is still waiting for them
    pub fn finish_listing_tools(
        &mut self,
        index: usize,
        address: String,
        server: String,
        result: Result<Vec<crate::mcp::Tool>, String>,
    ) {
        if self.tabs[index].url_input != address {
            return;
        }
        match result {
            Ok(tools) => {
                let html = crate::mcp::tools_page_html(&server, &tools);
                let host = Url::parse(&server).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
                self.mcp_tools.insert(server, tools);
                self.show_generated_page(index, &address, &format!("Tools from {}", host), html);
            }
            Err(e) => {
                self.show_generated_page(
                    index,
                    &address,
                    "Tools unavailable",
                    crate::mcp::error_page_html(&server, &e),
                );
                self.tabs[index].status_message = e;
            }
        }
    }

    /// Turn a form on the tools page into a tool call waiting for confirmation
    pub(super) fn prepare_tool_call(&mut self, form: usize, tool_name: &str) {
        let tab = &self.tabs[self.active_tab_index];
        let Some(server) = crate::mcp::server_of(&tab.url_input) else {
            return;
        };
        let Some(tool) =
            self.mcp_tools.get(&server).and_then(|tools| tools.iter().find(|t| t.name == tool_name)).cloned()
        else {
            self.current_tab().status_message = String::from("That tool is no longer listed; reload the page");
            return;
        };
        let mut entered = HashMap::new();
        for (control, value) in tab.controls.iter().zip(&tab.control_values) {
            if control.form != Some(form) || control.name.is_empty() {
                continue;
            }
            let field = match (&control.kind, value) {
                (ControlKind::Checkbox, ControlValue::Checked(on)) => crate::mcp::FieldValue::Checked(*on),
                (_, ControlValue::Text(text)) => crate::mcp::FieldValue::Text(text.clone()),
                (ControlKind::Select { options }, ControlValue::Selected(i)) => {
                    crate::mcp::FieldValue::Text(options.get(*i).map(|o| o.value.clone()).unwrap_or_default())
                }
                _ => continue,
            };
            entered.insert(control.name.clone(), field);
        }
        let tab = self.current_tab();
        match crate::mcp::arguments(&tool, &entered) {
            Ok(arguments) => {
                tab.tool_prompt = Some(crate::models::ToolPrompt {
                    server,
                    warning: tool.warning(),
                    tool: tool.name,
                    title: tool.title,
                    arguments,
                });
                tab.status_message = String::from("Check what will be sent: y runs the tool, n cancels");
            }
            Err(e) => tab.status_message = e,
        }
    }

    /// Run the confirmed tool call
    pub fn run_tool(&mut self) {
        let tab = self.current_tab();
        let Some(prompt) = tab.tool_prompt.take() else {
            return;
        };
        tab.status_message = format!("Running {}...", prompt.title);
        let id = tab.id;
        let tx = self.tx.clone();
        let http = self.network_manager.get_client(self.i2p_mode).clone();
        let mcp = Arc::clone(&self.mcp);
        tokio::spawn(async move {
            let result = mcp.call_tool(&http, &prompt.server, &prompt.tool, prompt.arguments).await;
            let _ = tx.send(NetworkResponse::ToolFinished(id, prompt.server, prompt.title, result)).await;
        });
    }

    /// Keep a tool's result, and show it if the tab is still on that server's tools page
    pub fn finish_tool(
        &mut self,
        index: usize,
        server: String,
        title: String,
        result: Result<serde_json::Value, String>,
    ) {
        self.next_result_id += 1;
        let id = self.next_result_id;
        let html = crate::mcp::result_page_html(&server, &title, &result);
        self.tool_results.push_back((id, title.clone(), html.clone()));
        if self.tool_results.len() > 20 {
            self.tool_results.pop_front();
        }
        let address = format!("{}{}", crate::mcp::RESULT_PAGE, id);
        let tab = &mut self.tabs[index];
        if tab.url_input != crate::mcp::tools_address(&server) {
            tab.status_message = format!("{} finished; type {} to see the result", title, address);
            return;
        }
        let entry = tab.history_entry(tab.url_input.clone());
        tab.history.push(entry);
        tab.forward_history.clear();
        self.show_generated_page(index, &address, &title, html);
    }
}

//! The I2P router rynx started (i2pd): how far it has got, on the status line and the settings
//! page. Starting and stopping it happen outside the app, before the sandbox and at exit.

use super::*;
use crate::i2pd::{Progress, RouterState};
use crate::network::escape_html;

/// How often the router's console is asked how it's doing
const POLL_SECS: u64 = 10;

impl App {
    /// Follow the progress of a router rynx started, until rynx quits
    pub fn watch_router(&mut self) {
        let RouterState::Started { console_port: Some(port), .. } = self.router_state else { return };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            loop {
                let progress = crate::i2pd::progress(port).await;
                if tx.send(NetworkResponse::RouterProgress(progress)).await.is_err() {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_secs(POLL_SECS)).await;
            }
        });
    }

    pub fn finish_router_progress(&mut self, progress: Option<Progress>) {
        let ready = progress.as_ref().is_some_and(Progress::ready);
        self.router_progress = progress;
        if ready && !std::mem::replace(&mut self.router_ready_announced, true) {
            self.current_tab().status_message = String::from(
                "The I2P router is ready. Until its address book arrives (a few more minutes the first \
                 time), use a jump service for .i2p names it doesn't know yet.",
            );
        }
    }

    /// The router part of the settings page
    pub(super) fn router_html(&self) -> String {
        let s = &self.settings;
        let status = match &self.router_state {
            RouterState::NotManaged => {
                String::from("rynx uses whatever I2P router is running at the I2P proxy's address.")
            }
            RouterState::RemoteProxy => {
                String::from("The I2P proxy isn't on this computer, so there's no router here for rynx to start.")
            }
            RouterState::AlreadyRunning => String::from(
                "An I2P router was already running at the I2P proxy's address when rynx started, so rynx left it alone.",
            ),
            RouterState::NotFound => format!("i2pd isn't installed: {}.", crate::i2pd::install_hint()),
            RouterState::Clearweb => {
                String::from("rynx is set to always use the clearweb, so it didn't start the router.")
            }
            RouterState::Failed(why) => format!("The router didn't start: {}", why),
            RouterState::Started { pid, console_port, keep } => {
                let progress = match (&self.router_progress, console_port) {
                    (Some(progress), _) => progress.describe(),
                    (None, Some(_)) => String::from("starting"),
                    (None, None) => String::from("running (its console's port was taken, so its progress isn't known)"),
                };
                let after = if *keep { "keeps running after rynx quits" } else { "stops when rynx quits" };
                format!("rynx started i2pd (process {}): {}. It {}.", pid, progress, after)
            }
        };
        let checkbox = |name: &str, on: bool, label: &str| {
            let checked = if on { " checked" } else { "" };
            format!("<p><input type=\"checkbox\" name=\"{name}\" value=\"on\"{checked}> {label}</p>")
        };
        let mut html = format!(
            "<h2>I2P router</h2><p><b>{}</b></p>\
             <p>A router lets rynx reach I2P; it doesn't keep rynx there. For that, choose I2P only under \
             Network.</p>",
            escape_html(&status)
        );
        html.push_str(&checkbox(
            "i2p_router",
            s.start_i2p_router,
            "Start i2pd when rynx starts, if no router is running (it takes a few minutes to join the network)",
        ));
        html.push_str(&checkbox(
            "i2p_router_keep",
            s.keep_i2p_router,
            "Keep it running after rynx quits (ready sooner next time, and it carries others' traffic, as routers do)",
        ));
        html.push_str(&format!(
            "<p><b>i2pd program</b>: empty to look on PATH and where package managers install it.<br>\
             <input type=\"text\" name=\"i2pd_path\" size=\"80\" value=\"{}\"></p>\
             <p>These take effect when rynx next starts: the router is started before the sandbox, so it isn't confined by it.</p>",
            escape_html(&s.i2pd_path.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default())
        ));
        html
    }
}

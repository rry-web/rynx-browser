//! The I2P router rynx started (i2pd): how far it has got, on the status line, the settings page
//! and about:i2p, the guide to the router's console, torrents and mail. Starting and stopping it
//! happen outside the app, before the sandbox and at exit.

use super::*;
use crate::i2pd::{Progress, RouterState};
use crate::network::escape_html;

/// How often the router's console is asked how it's doing
const POLL_SECS: u64 = 10;

/// The guide to I2P's other uses: the router's console, torrents and mail
pub const I2P_PAGE: &str = "about:i2p";

/// The Java I2P router's console, where its torrent (I2PSnark) and mail (SusiMail) apps live
const JAVA_I2P_CONSOLE: &str = "http://127.0.0.1:7657/";

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

    /// What the router is doing, in a sentence
    fn router_status(&self) -> String {
        match &self.router_state {
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
        }
    }

    /// The router part of the settings page
    pub(super) fn router_html(&self) -> String {
        let s = &self.settings;
        let status = self.router_status();
        let checkbox = |name: &str, on: bool, label: &str| {
            let checked = if on { " checked" } else { "" };
            format!("<p><input type=\"checkbox\" name=\"{name}\" value=\"on\"{checked}> {label}</p>")
        };
        let mut html = format!(
            "<h2>I2P router</h2><p><b>{}</b></p>\
             <p>A router lets rynx reach I2P; it doesn't keep rynx there. For that, choose I2P only under \
             Network. <a href=\"{I2P_PAGE}\">I2P</a> covers the router's console, torrents and mail.</p>",
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

    /// about:i2p: reaching the router's console, and getting torrents and mail over I2P with the
    /// programs made for them
    pub(super) fn i2p_page_html(&self) -> String {
        let console = match self.router_state {
            RouterState::Started { console_port: Some(port), .. } => format!("http://127.0.0.1:{}/", port),
            _ => format!("http://127.0.0.1:{}/", crate::i2pd::CONSOLE_PORT),
        };
        let tunnels = match crate::i2pd::data_dir() {
            Some(dir) => format!(
                "the <b>tunnels.conf</b> in your i2pd's folder (for the i2pd rynx starts, {})",
                escape_html(&dir.join("tunnels.conf").to_string_lossy())
            ),
            None => String::from("the <b>tunnels.conf</b> in your i2pd's folder"),
        };
        let link = |url: &str| format!("<a href=\"{0}\">{0}</a>", escape_html(url));
        format!(
            "<title>I2P</title><h1>I2P</h1><p><b>{status}</b></p>\
             <p>rynx browses I2P sites; for torrents and mail, use the programs made for them, through the \
             same router. In I2P mode rynx opens this computer's addresses, like those below, when you type \
             them or open them from here; I2P sites can't link to them.</p>\
             <h2>The router's console</h2>\
             <p>i2pd: {console}<br>Java I2P: {java}</p>\
             <h2>Torrents</h2>\
             <p>A magnet link on a page is copied when you open it, to paste into a torrent client.</p>\
             <p><b>Java I2P</b> comes with one, I2PSnark: {snark}</p>\
             <p><b>With i2pd</b>, use a client that talks to the router over SAM, such as XD (which has a \
             web page of its own, to open in rynx), qBittorrent or BiglyBT (with its I2P plugin). SAM is \
             off in the i2pd rynx starts; for torrents, run your own i2pd with <b>sam.enabled = true</b> in \
             its i2pd.conf. rynx uses a router that's already running and leaves it alone.</p>\
             <h2>Mail</h2>\
             <p><b>Java I2P</b> comes with SusiMail: {susi}</p>\
             <p><b>With i2pd</b>, add these to {tunnels}, restart it, and point any mail client (aerc and \
             neomutt work in a terminal) at 127.0.0.1: port 7659 to send, 7660 to receive. Accounts are made \
             at {postman}.</p>\
             <pre>[smtp-postman]\ntype = client\naddress = 127.0.0.1\nport = 7659\n\
             destination = smtp.postman.i2p\ndestinationport = 25\nkeys = transient-smtp\n\n\
             [pop3-postman]\ntype = client\naddress = 127.0.0.1\nport = 7660\n\
             destination = pop.postman.i2p\ndestinationport = 110\nkeys = transient-pop3</pre>",
            status = escape_html(&self.router_status()),
            console = link(&console),
            java = link(JAVA_I2P_CONSOLE),
            snark = link(&format!("{JAVA_I2P_CONSOLE}i2psnark/")),
            susi = link(&format!("{JAVA_I2P_CONSOLE}susimail/")),
            postman = link("http://hq.postman.i2p/"),
        )
    }
}

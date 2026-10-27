//! The update page (about:update, U): what's new, and installing it at the next start. Checks
//! and downloads go to GitHub over the clearweb, so never in I2P mode. Only this browser-made
//! page's buttons can start a download, as with the settings page.

use super::*;
use crate::network::escape_html;
use crate::update::{Installed, Release, UPDATE_PAGE};

impl App {
    /// Why an update check can't happen now, if it can't
    fn update_check_blocked(&self) -> Option<&'static str> {
        if self.settings.network == crate::settings::NetworkMode::I2p {
            Some("rynx is locked to I2P, and checking for updates would leave it (GitHub is on the clearweb)")
        } else if self.i2p_mode {
            Some("Leave I2P mode (p) to check for updates: GitHub is on the clearweb")
        } else {
            None
        }
    }

    /// Ask GitHub for a newer release, in the background
    pub fn start_update_check(&mut self, asked: bool) {
        if let Some(reason) = self.update_check_blocked() {
            if asked {
                self.current_tab().status_message = reason.to_string();
            }
            return;
        }
        let tab_id = self.current_tab().id;
        let client = self.network_manager.get_client(false).clone();
        let api = self.update_api.clone();
        let betas = self.settings.beta_updates;
        let tx = self.tx.clone();
        if asked {
            self.current_tab().status_message = String::from("Checking for updates...");
        }
        tokio::spawn(async move {
            let result = crate::update::check(&client, &api, betas).await;
            let _ = tx.send(NetworkResponse::UpdateChecked(tab_id, asked, result)).await;
        });
    }

    /// Check once a day, if the settings allow, noting when
    pub fn check_for_updates_if_due(&mut self) {
        if !self.settings.check_updates || self.update_check_blocked().is_some() {
            return;
        }
        let Some(dir) = self.config_dir.clone() else { return };
        let file = dir.join(crate::update::LAST_CHECK_FILE);
        let last: u64 = std::fs::read_to_string(&file).ok().and_then(|t| t.trim().parse().ok()).unwrap_or(0);
        let now = crate::cache::now();
        if now.saturating_sub(last) < crate::update::CHECK_INTERVAL_SECS {
            return;
        }
        let _ = std::fs::write(&file, now.to_string());
        self.start_update_check(false);
    }

    /// The answer from GitHub
    pub fn finish_update_check(&mut self, index: usize, asked: bool, result: Result<Option<Release>, String>) {
        let message = match &result {
            Ok(Some(release)) => Some(format!("rynx {} is available: U shows what's new", release.version)),
            Ok(None) if asked => Some(format!("rynx {} is the newest version", crate::update::current_version())),
            Err(e) if asked => Some(format!("Couldn't check for updates: {}", e)),
            _ => None,
        };
        if let Ok(found) = result {
            self.update_found = found;
        }
        if self.tabs[index].url_input == UPDATE_PAGE {
            self.show_update_page();
        }
        if let Some(message) = message {
            self.tabs[index].status_message = message;
        }
    }

    pub(super) fn show_update_page(&mut self) {
        let html = self.update_page_html();
        self.show_generated_page(self.active_tab_index, UPDATE_PAGE, "Updates", html);
    }

    fn update_page_html(&self) -> String {
        let current = crate::update::current_version();
        let mut html = format!(
            "<title>Updates</title><h1>Updates</h1><p>This is rynx {} for {}.</p>",
            current,
            escape_html(crate::update::TARGET)
        );
        if let Some(note) = &self.update_note {
            html.push_str(&format!("<p><b>{}</b></p>", escape_html(note)));
        }
        let pending = self.update_dir.as_deref().and_then(crate::update::pending);
        if let Some(pending) = &pending {
            html.push_str(&format!(
                "<p>rynx {} is downloaded and checked. It's installed the next time rynx starts.</p>\
                 <form action=\"about:update-do\"><input type=\"submit\" name=\"cancel\" value=\"Don't install it\"></form>",
                escape_html(&pending.version)
            ));
        }
        match &self.update_found {
            Some(release) if pending.is_none() => {
                html.push_str(&format!("<h2>rynx {} is available</h2>", release.version));
                html.push_str(&match &self.installed {
                    Installed::Updatable(_) => "<form action=\"about:update-do\"><input type=\"submit\" name=\"install\" \
                         value=\"Download it, and install it when rynx next starts\"></form>\
                         <p>It's installed only if it carries the release key's signature.</p>"
                        .to_string(),
                    Installed::Cargo => format!(
                        "<p>This copy was installed with Cargo; update it with:</p><pre>cargo install rynx --version {}</pre>",
                        release.version
                    ),
                    Installed::Homebrew => "<p>This copy was installed with Homebrew; update it with:</p><pre>brew upgrade rynx</pre>"
                        .to_string(),
                    Installed::Elsewhere(why) => format!(
                        "<p>rynx can't update itself here ({}). Download it from <a href=\"{}\">its release page</a>.</p>",
                        escape_html(why),
                        escape_html(&release.page)
                    ),
                });
                html.push_str(&format!(
                    "<p><a href=\"{}\">The release on GitHub</a></p><h2>What's new</h2>",
                    escape_html(&release.page)
                ));
                // The notes are Markdown, shown as text
                for paragraph in release.notes.split("\n\n").filter(|p| !p.trim().is_empty()).take(80) {
                    html.push_str(&format!("<p>{}</p>", escape_html(paragraph.trim()).replace('\n', "<br>")));
                }
            }
            Some(_) => {}
            None => html.push_str("<p>No newer version is known yet.</p>"),
        }
        let channel = if self.settings.beta_updates { "betas included" } else { "stable releases only" };
        let schedule = if self.settings.check_updates { "once a day" } else { "only when asked" };
        html.push_str(&format!(
            "<form action=\"about:update-do\"><input type=\"submit\" name=\"check\" value=\"Check now\"></form>\
             <p>rynx checks {} ({}); the settings page (S) changes that.</p>",
            schedule, channel
        ));
        html
    }

    /// A button on the update page
    pub(super) fn update_form(&mut self, submitter: Option<usize>) {
        let tab = &self.tabs[self.active_tab_index];
        let pressed = submitter.and_then(|i| tab.controls.get(i)).map(|c| c.name.clone()).unwrap_or_default();
        match pressed.as_str() {
            "check" => self.start_update_check(true),
            "cancel" => {
                if let Some(dir) = &self.update_dir {
                    crate::update::cancel(dir);
                }
                self.show_update_page();
                self.current_tab().status_message = String::from("The downloaded update won't be installed");
            }
            "install" => self.download_update(),
            _ => {}
        }
    }

    fn download_update(&mut self) {
        let (Some(release), Some(dir)) = (self.update_found.clone(), self.update_dir.clone()) else { return };
        if let Some(reason) = self.update_check_blocked() {
            self.current_tab().status_message = reason.to_string();
            return;
        }
        if !matches!(self.installed, Installed::Updatable(_)) {
            return;
        }
        let tab_id = self.current_tab().id;
        let client = self.network_manager.get_download_client(false).clone();
        let key = self.release_key;
        let tx = self.tx.clone();
        self.current_tab().status_message = format!("Downloading rynx {}...", release.version);
        tokio::spawn(async move {
            let result = crate::update::download(&client, &release, &dir, &key).await.map(|p| p.version);
            let _ = tx.send(NetworkResponse::UpdateDownloaded(tab_id, result)).await;
        });
    }

    pub fn finish_update_download(&mut self, index: usize, result: Result<String, String>) {
        let message = match result {
            Ok(version) => format!("rynx {} is downloaded and checked: it's installed when rynx next starts", version),
            Err(e) => format!("Couldn't update: {}", e),
        };
        if self.tabs[index].url_input == UPDATE_PAGE {
            self.show_update_page();
        }
        self.tabs[index].status_message = message;
    }
}

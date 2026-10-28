//! Cookies for the sites you allow: C on a page lets its site keep them (so you stay signed in)
//! or forgets them; about:cookies lists the sites. Only this browser-made page's buttons can
//! forget a site, never a web page's form.

use super::*;
use crate::cookies::{COOKIES_PAGE, site_of};
use crate::network::escape_html;

impl App {
    /// The web site of the page in the current tab, if it's one cookies apply to
    fn cookie_host(&mut self) -> Option<String> {
        let tab = self.current_tab();
        let url = url::Url::parse(tab.page_url.as_deref()?).ok()?;
        matches!(url.scheme(), "http" | "https").then(|| url.host_str().map(str::to_string)).flatten()
    }

    /// C: let this page's site keep cookies, or forget them
    pub fn toggle_cookies(&mut self) {
        let Some(host) = self.cookie_host() else {
            self.current_tab().status_message = String::from("Only web sites keep cookies");
            return;
        };
        let jar = Arc::clone(self.network_manager.cookies(self.i2p_mode));
        let message = if jar.allowed(&host) {
            let site = site_of(&host);
            match jar.forget(&site) {
                Ok(()) => format!("{} can't keep cookies now; its cookies are forgotten (signed out)", site),
                Err(e) => e,
            }
        } else {
            match jar.allow(&host) {
                Ok(site) if jar.is_saved() => {
                    format!("{} may keep cookies: sign in, and you'll stay signed in. C again forgets them.", site)
                }
                Ok(site) => format!("{} may keep cookies until rynx quits (I2P cookies are never saved)", site),
                Err(e) => e,
            }
        };
        self.current_tab().status_message = message;
    }

    pub(super) fn cookies_page_html(&self) -> String {
        let mut html = String::from(
            "<title>Cookies</title><h1>Cookies</h1><p>rynx keeps no cookies, except for the sites listed here, \
             so you can stay signed in to them. Press <b>C</b> on a site's page to add it (before signing in) \
             or to forget it. Cookies meant to outlast the session are saved in the settings folder, readable \
             only by you; I2P sites are allowed until rynx quits, and their cookies are never saved.</p>",
        );
        for (heading, i2p) in [("Clearweb", false), ("I2P (until rynx quits)", true)] {
            let sites = self.network_manager.cookies(i2p).sites();
            if sites.is_empty() && i2p {
                continue;
            }
            html.push_str(&format!("<h2>{}</h2>", heading));
            if sites.is_empty() {
                html.push_str("<p>No sites yet.</p>");
                continue;
            }
            html.push_str(&format!(
                "<form action=\"{}-do\"><table><tr><th>Site</th><th>Cookies</th><th></th></tr>",
                COOKIES_PAGE
            ));
            for (site, count) in sites {
                let name = if i2p { "forget_i2p" } else { "forget" };
                html.push_str(&format!(
                    "<tr><td>{}</td><td>{}</td><td><button type=\"submit\" name=\"{}\" value=\"{}\">Forget</button></td></tr>",
                    escape_html(&site),
                    count,
                    name,
                    escape_html(&site)
                ));
            }
            html.push_str("</table></form>");
        }
        html
    }

    /// A Forget button on the cookies page
    pub(super) fn cookies_form(&mut self, submitter: Option<usize>) {
        let tab = &self.tabs[self.active_tab_index];
        let Some(control) = submitter.and_then(|i| tab.controls.get(i)).cloned() else { return };
        let i2p = match control.name.as_str() {
            "forget" => false,
            "forget_i2p" => true,
            _ => return,
        };
        let message = match self.network_manager.cookies(i2p).forget(&control.value) {
            Ok(()) => format!("{}'s cookies are forgotten", control.value),
            Err(e) => e,
        };
        self.show_internal_page(COOKIES_PAGE);
        self.current_tab().status_message = message;
    }

    /// On a page asking for a password, from a site that can't keep cookies: say how to stay
    /// signed in, since without cookies a sign-in lasts only for that one page
    pub fn hint_cookies_for_sign_in(&mut self, tab_index: usize) {
        let tab = &self.tabs[tab_index];
        let asks_password = tab.controls.iter().any(|c| c.kind == crate::forms::ControlKind::Text { password: true });
        let Some(host) = tab
            .page_url
            .as_deref()
            .and_then(|u| url::Url::parse(u).ok())
            .filter(|u| matches!(u.scheme(), "http" | "https"))
            .and_then(|u| u.host_str().map(str::to_string))
        else {
            return;
        };
        if asks_password && !self.network_manager.cookies(self.i2p_mode).allowed(&host) {
            self.tabs[tab_index].status_message = format!(
                "To stay signed in to {}, press C before signing in (it lets the site keep cookies)",
                site_of(&host)
            );
        }
    }
}

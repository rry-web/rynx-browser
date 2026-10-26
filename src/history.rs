//! Pages visited, newest last. Kept in memory for the session, and saved to a plain text file in
//! the config folder only when the user turns it on (`history = on`): one "time<TAB>URL<TAB>title"
//! per line, with the time in seconds since 1970.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub const HISTORY_FILE: &str = "history.txt";

/// The address of the history page
pub const HISTORY_PAGE: &str = "about:history";

/// Older visits are forgotten beyond this many pages
pub const MAX_HISTORY: usize = 2000;

#[derive(Clone, Debug, PartialEq)]
pub struct Visit {
    /// Seconds since 1970 (UTC)
    pub time: u64,
    pub url: String,
    pub title: String,
    /// Never written to disk (visits made in I2P mode)
    pub private: bool,
}

#[derive(Debug, Default)]
pub struct History {
    pub entries: Vec<Visit>,
    /// Where it's saved; None keeps it in memory only (history saving off, or tests)
    path: Option<PathBuf>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Tabs and line breaks would corrupt the file format
fn clean(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

impl History {
    /// History saved to `path`, starting with what's already there
    pub fn load(path: PathBuf) -> History {
        let mut entries: Vec<Visit> = std::fs::read_to_string(&path)
            .map(|text| {
                text.lines()
                    .filter_map(|line| {
                        let mut fields = line.splitn(3, '\t');
                        let time = fields.next()?.trim().parse().ok()?;
                        let url = fields.next()?.trim();
                        let title = fields.next().unwrap_or("").trim();
                        (!url.is_empty()).then(|| Visit {
                            time,
                            url: url.to_string(),
                            title: title.to_string(),
                            private: false,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        if entries.len() > MAX_HISTORY {
            entries.drain(..entries.len() - MAX_HISTORY);
        }
        History { entries, path: Some(path) }
    }

    /// Whether visits are saved to disk
    pub fn is_saved(&self) -> bool {
        self.path.is_some()
    }

    /// Record a visit to `url`, moving an earlier visit to the same page to the end. `private`
    /// visits are listed for this session but never saved.
    pub fn record(&mut self, url: &str, title: &str, private: bool) -> Result<(), String> {
        let url = clean(url);
        self.entries.retain(|v| v.url != url);
        self.entries.push(Visit { time: now(), url, title: clean(title), private });
        if self.entries.len() > MAX_HISTORY {
            self.entries.remove(0);
        }
        if private { Ok(()) } else { self.save() }
    }

    /// Forget every visit, on disk too
    pub fn clear(&mut self) -> Result<(), String> {
        self.entries.clear();
        self.save()
    }

    /// Write the file atomically: a crash mid-write leaves the old file, not a truncated one
    fn save(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let text: String = self
            .entries
            .iter()
            .filter(|v| !v.private)
            .map(|v| format!("{}\t{}\t{}\n", v.time, v.url, v.title))
            .collect();
        let temporary = path.with_extension("txt.tmp");
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't save history: {}", e))?;
        }
        std::fs::write(&temporary, text)
            .and_then(|_| std::fs::rename(&temporary, path))
            .map_err(|e| format!("Couldn't save history: {}", e))
    }

    /// The history page: newest first, under a heading per day
    pub fn page_html(&self) -> String {
        use crate::network::escape_html;
        let mut html = String::from("<title>History</title><h1>History</h1>");
        if self.entries.is_empty() {
            html.push_str("<p>No pages visited yet.</p>");
        } else {
            let mut day = String::new();
            for visit in self.entries.iter().rev() {
                let (date, time) = utc_date_time(visit.time);
                if date != day {
                    if !day.is_empty() {
                        html.push_str("</ul>");
                    }
                    html.push_str(&format!("<h2>{}</h2><ul>", date));
                    day = date;
                }
                let title = if visit.title.is_empty() { &visit.url } else { &visit.title };
                html.push_str(&format!(
                    "<li>{} <a href=\"{}\">{}</a></li>",
                    time,
                    escape_html(&visit.url),
                    escape_html(title)
                ));
            }
            html.push_str("</ul><p>Times are UTC. Press <b>x</b> twice to clear the history.</p>");
        }
        if !self.is_saved() {
            html.push_str(
                "<p>History is kept only until you quit. To keep it, add <b>history = on</b> to \
                 settings.conf.</p>",
            );
        }
        html
    }
}

/// "YYYY-MM-DD" and "HH:MM" for a time in seconds since 1970
fn utc_date_time(seconds: u64) -> (String, String) {
    let days = (seconds / 86_400) as i64;
    let in_day = seconds % 86_400;
    // Civil date from days since 1970 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (format!("{:04}-{:02}-{:02}", year, month, day), format!("{:02}:{:02}", in_day / 3600, in_day % 3600 / 60))
}

#[cfg(test)]
mod tests {
    use super::utc_date_time;

    #[test]
    fn dates_are_converted() {
        assert_eq!(utc_date_time(0), ("1970-01-01".into(), "00:00".into()));
        assert_eq!(utc_date_time(951_782_400), ("2000-02-29".into(), "00:00".into()));
        assert_eq!(utc_date_time(1_793_016_000 + 3_723), ("2026-10-26".into(), "13:02".into()));
    }
}

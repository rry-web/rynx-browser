/// How rynx introduces itself to websites. Honest, like Lynx and w3m: bot walls such as Anubis
/// challenge browsers that claim to be "Mozilla", and let text browsers that don't straight through.
/// Changeable with `user_agent` in settings.conf.
pub const USER_AGENT: &str = concat!("rynx/", env!("CARGO_PKG_VERSION"), " (+https://github.com/rry-web/rynx-browser)");
/// Sent in I2P mode whatever the setting, so I2P visits don't stand out: it's what the I2P router's
/// HTTP proxy puts in requests it can see, and HTTPS requests through an outproxy it can't rewrite.
pub const I2P_USER_AGENT: &str = "MYOB/6.66 (AN/ON)";

// Network configuration
pub const I2P_PROXY_URL: &str = "http://127.0.0.1:4444";
pub const BROWSING_TIMEOUT_SECS: u64 = 100;
pub const DOWNLOAD_TIMEOUT_SECS: u64 = 3000;
pub const DOWNLOAD_PROGRESS_INTERVAL_MS: u64 = 100;

// Channel capacity
pub const CHANNEL_CAPACITY: usize = 10;

// UI layout constants
pub const TAB_BAR_HEIGHT: u16 = 3;
pub const URL_BAR_HEIGHT: u16 = 3;
pub const UI_ROW_OFFSET: u16 = 7;
pub const UI_HEIGHT_OFFSET: u16 = 8;
pub const UI_BORDER_WIDTH: usize = 2;
pub const MOUSE_SCROLL_LINES: usize = 3;

// File size limits
pub const MAX_PAGE_SIZE_BYTES: u64 = 10 * 1024 * 1024; // 10MB

// Tab navigation
pub const DEFAULT_TAB_INDEX: usize = 0;
pub const INITIAL_TAB_ID: usize = 0;
pub const INITIAL_ID_COUNTER: usize = 1;

// I2P jump services
/// Jump services, asked in turn for an .i2p name the router doesn't know; `{}` is the name. By
/// their b32 addresses, as i2pd lists them, so they work before the address book knows them
pub const JUMP_SERVICES: &[&str] = &[
    // reg.i2p
    "http://shx5vqsw7usdaunyzr2qmes2fq37oumybpudrd4jjj4e4vk4uusa.b32.i2p/jump/{}",
    // notbob.i2p
    "http://nytzrhrjjfsutowojvxi7hphesskpqqr65wpistz6wa7cpajhp7a.b32.i2p/cgi-bin/jump.cgi?q={}",
    // stats.i2p
    "http://kqypgjpjwrphnzebod5ev3ts2vtii6e5tntrg4rnfijqc7rypldq.b32.i2p/cgi-bin/jump.cgi?a={}",
];

// Event polling
pub const EVENT_POLL_TIMEOUT_MS: u64 = 10;
// How long to wait for in-flight input after mouse capture is disabled on exit
pub const EXIT_DRAIN_TIMEOUT_MS: u64 = 20;

// Redirect policy
pub const MAX_REDIRECTS: usize = 10;

// Search URLs
pub const MARGINALIA_SEARCH_URL: &str = "https://old-search.marginalia.nu/search?";
/// Default search engine; {query} is replaced by the search words. Changeable in settings.conf.
pub const DEFAULT_SEARCH_URL: &str = "https://old-search.marginalia.nu/search?query={query}";

/// Search engines offered on the settings page: (name, label, search URL). Both work without
/// JavaScript; Marginalia's older interface is the one it recommends for text browsers.
pub const SEARCH_ENGINES: &[(&str, &str, &str)] = &[
    ("marginalia", "Marginalia: independent, finds the non-commercial web", DEFAULT_SEARCH_URL),
    ("duckduckgo", "DuckDuckGo (its HTML version): the wider web", "https://html.duckduckgo.com/html/?q={query}"),
];

pub const DOWNLOAD_PROMPT_WIDTH_DIVISOR: u16 = 2; // 50% of terminal width
pub const DOWNLOAD_PROMPT_X_DIVISOR: u16 = 4; // Centered at 25% offset
pub const DOWNLOAD_PROMPT_Y_DIVISOR: u16 = 2; // Vertical Center
pub const DOWNLOAD_PROMPT_HEIGHT: u16 = 9;
pub const DOWNLOAD_PROMPT_Y_OFFSET: u16 = 4; // Vertical centering offset, height / 2
pub const DOWNLOAD_PROMPT_BUTTON_ROW_OFFSET: u16 = 5; // Row inside the prompt for Y/N

// User Agent constants
pub const USER_AGENT_BROWSING: &str = "RustBrowser/0.1.0 reqwest/0.12";
pub const USER_AGENT_DOWNLOAD: &str = "RynxBrowser/0.1.0";

// Network configuration
pub const I2P_PROXY_URL: &str = "http://127.0.0.1:4444";
pub const BROWSING_TIMEOUT_SECS: u64 = 100;
pub const DOWNLOAD_TIMEOUT_SECS: u64 = 3000;

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
pub const JUMP_SERVICES: &[&str] = &[
    "http://i2p-projekt.i2p/jump/",
    "http://stats.i2p/jump/",
    "http://reg.i2p/jump/",
];

// Event polling
pub const EVENT_POLL_TIMEOUT_MS: u64 = 10;

// Redirect policy
pub const MAX_REDIRECTS: usize = 10;

// Search URLs
pub const MARGINALIA_SEARCH_URL: &str = "https://search.marginalia.nu/search?";

# Changelog

## [Unreleased]

### Added
- History: `s` lists the pages you've visited, newest first by day. It's kept in memory unless
  `history = on` is set in settings.conf, which saves it to `history.txt`. Visits in I2P mode
  and passwords in addresses are never saved. `x` twice on the history page clears it.
- Gemini: `gemini://` addresses, with links, headings, lists, quotes and preformatted text;
  input prompts become a form. Certificates are trusted on first use and remembered in
  `gemini_known_hosts.txt`; a changed certificate is refused unless CA-signed.
- Gopher: `gopher://` menus (kept as fixed-width text, with item types marked), text documents,
  and searches.
- RSS and Atom feeds open as a page listing each post with its date and content, and pages that
  announce a feed show a `Feeds:` link at the top.
- Site tools (MCP): a page announcing an MCP server with `<link rel="mcp">` shows a `Tools:`
  link, which lists the server's tools as forms. Running one shows exactly what will be sent and
  needs `y` to confirm; the result opens as a page. Works with any MCP server using the
  Streamable HTTP transport (JSON or event-stream answers), including `about:tools?server=...`
  typed by hand.

### Fixed
- The download confirmation box now takes every key until answered: other keys (even `q`) used
  to act on the page underneath while it was open.

### Security
- Tool servers must be on the page's own site and use HTTPS, can't be on the local network, and
  can't redirect; web pages can't submit forms to tools or open tool results.
- Public pages can't link to Gopher or Gemini on the local network, and Gopher addresses with
  line breaks are refused, so a link can't send commands to local services like Redis.
- IP addresses are recognised in every kind of address: `gopher://127.0.0.1/` (or disguises like
  `2130706433`) counts as local, which the URL parser left as a plain name for non-web schemes.
- Gemini and Gopher are refused in I2P mode, since they don't go through the I2P proxy.
- Feeds are parsed without expanding XML entities, so entity bombs can't exhaust memory.

## [0.2.0-beta.1]

First beta build, with pre-built binaries for Linux, macOS and Windows. Please report problems as
GitHub issues; [TESTING.md](https://github.com/rry-web/rynx-browser/blob/main/TESTING.md) has a checklist of things worth trying.

### Rendering
- Headings h1–h6, numbered and nested lists, blockquotes, definition lists, `<details>`.
- Tables laid out in aligned columns, falling back to `Header: value` rows on narrow terminals.
- Site menus (lists of links in `<nav>`, footers, or marked as menus) are drawn on one line,
  `Home · News · Sport`, with submenus on labelled lines (`News: UK · Asia`).
- Inline code no longer breaks paragraphs; code blocks keep their lines.
- Correct spacing around inline tags; `&nbsp;` preserved.
- Plain-text files (`.txt`, JSON, source code) shown as text, with clickable URLs.
- Images, PDFs and other non-text files offered for download instead of shown as garbage;
  links only go straight to the download prompt for known file types.
- Embedded video, audio and frames shown as links; `<noscript>` content displayed.
- Pages are parsed and laid out on a background thread: slow pages never freeze the UI.

### Forms
- Search boxes and other forms work: text and password fields, text areas, checkboxes, radio
  buttons, choices and buttons, submitted with GET or POST.
- The bottom bar shows where a form sends what you type; GET forms never put a password in the
  address; a public page's form can't send data to your local network.
- Links and forms that disguise their real site with a user name (`https://bank.com@evil.example`)
  are blocked and flagged in the preview; passwords in typed addresses are hidden on screen and
  never saved in bookmarks.

### Navigation
- Back and Forward (`Right` / `L`) return to where you were on each page; `r` reloads.
- Paging keys: `PageUp`/`PageDown`, `Ctrl+u`/`Ctrl+d`, `g`/`G`.
- In-page links (`#chapter-1`) jump within the page instead of reloading it.
- Redirects are tracked: the address bar shows where you ended up.
- Back after typing a URL returns to the previous page; Esc cancels editing.
- The selected link's full destination is shown below the page.
- Scrolling stops at the end of the page, and the cursor follows the view.
- Touchpad scrolling keeps up, and quitting is instant.

### Settings and bookmarks
- `settings.conf` in the config folder: home page, search engine, I2P proxy, downloads folder.
  A commented template is written on first run.
- Bookmarks: `b` bookmarks the page (or removes it), `B` lists them (`about:bookmarks`).

### Downloads
- Much faster; filenames come from the URL without query strings.
- 404 error pages aren't saved; existing files aren't overwritten silently.
- Filenames are safe on every OS (no Windows device names like `CON`, no hidden files).
- Downloads are tagged with their origin for the OS's security checks.

### Security
- HTTPS everywhere, including redirects and downloads; certificates checked by the OS,
  including revocation (macOS, Windows).
- Linux filesystem sandbox (Landlock).
- Terminal escape sequences from pages are neutralised.
- Fixes for crashes and freezes triggered by malicious pages (deep nesting, nested lists,
  compression bombs) and for memory use on large pages.
- Non-web links (`mailto:`, `javascript:`, `file:`) are refused; `.i2p` sites are only
  contacted in I2P mode.
- Dependencies updated; no known vulnerabilities (`cargo audit`).

### Command line
- `--version`, `--help`, and `--no-sandbox` (Linux troubleshooting).

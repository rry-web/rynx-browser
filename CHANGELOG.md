# Changelog

## [0.2.0-beta.6]

Images, a page cache and snapshots, resumable downloads, and in-app updates: the first release
whose checksums are signed, so later releases can be installed from inside rynx (press `U`).
Copies of earlier betas need this one installed by hand.

### Changed
- A picture without a description now shows as `[image]` (so it can be loaded), unless it's
  marked decorative (`alt=""`) or is a 1–2 pixel tracking image.

### Added
- A page cache. Pages are kept in memory for the session: Back and Forward show them at once, a
  page the site says is fresh isn't fetched again, an older one is checked with the site
  (If-None-Match / If-Modified-Since) rather than downloaded, and no-store pages are never kept.
  When a site can't be reached, or answers with a server error (as the I2P router does for an
  unreachable site), the kept copy is shown with when it was saved. `r` always checks.
- An opt-in disk cache (`cache = on`, and `cache_i2p = on` for I2P pages), gzip-compressed in the
  cache folder, limited to `cache_size` megabytes (200 by default), oldest first out. On the
  settings page, with a Clear cache button.
- Snapshots of tracked sites: `T` tracks the current page's site, and each visit to one of its
  pages then keeps a dated, compressed snapshot when the page changed. `Z` lists a page's
  snapshots; opening one shows the page as it was, with working links, and `r` loads the live
  page. Snapshots can be deleted per site (asks twice).

- Images. Pages show placeholders until `I` loads their images (or `images = site` / `all` lets
  them load by themselves); `i` opens one full screen. Loaded images are drawn in the page
  (`image_display = inline`, the default) or only in the viewer, with Kitty graphics, iTerm2 images
  or Sixel where the terminal supports them, otherwise coloured text blocks (`image_protocol`
  chooses). Images go through the cache; lazy-loading `data-src`/`srcset` addresses are found;
  tracking pixels are skipped. Decoding uses pure-Rust decoders with size limits, big images are
  scaled down, SVG isn't supported, and public pages can't load images from the local network.
  Decoded images use at most 192 MB, least recently used out (decoded again from the cache when
  needed on screen).
- Resumable downloads. A download is written to `<name>.part` and renamed when complete; when the
  connection breaks it resumes from where it stopped (a range request, up to five times), and a
  download that still failed resumes the next time the same link is downloaded (the prompt says
  how much was saved). The ETag or Last-Modified goes with each resume (If-Range), so a file that
  changed on the server is downloaded afresh rather than spliced.
- Updates. rynx checks GitHub once a day (`updates = daily`, the default; never in I2P mode) and
  says when a newer version is out; `U` shows the release notes and, if you agree, downloads this
  platform's build, which is installed at the next start (before the sandbox) and started at once.
  The update is installed only if the release's `SHA256SUMS` carries a valid signature from the
  release key built into rynx and the download matches its signed checksum. Copies installed with
  Cargo or Homebrew get the command to update instead. `update_channel = stable` skips betas.
- Releases are signed: `SHA256SUMS.sig` (Ed25519), and each platform's bare program is published
  alongside the archives, for the updater.
- CI: `cargo deny` (vulnerabilities, licences compatible with the GPL, crates.io only) replaces
  `cargo audit`; a Rust 1.90 job checks the minimum version (raised from 1.88: the image decoders need 1.90); releases run formatting, lints and
  `cargo deny` before building, and publish to crates.io by trusted publishing (no stored token);
  Dependabot proposes dependency and action updates weekly.

### Fixed
- Reload on a page shown under an `about:` address reloads the page itself.

## [0.2.0-beta.5]

A settings page, with a lock to keep rynx on I2P (or off it), and archived copies of pages.

### Added
- A settings page (`S`, or `about:settings`): every setting as a form, checked like the settings
  file and applied as soon as it's saved (the downloads folder when rynx next starts), with links
  to history and bookmarks. Only the browser's own page can change settings, never a web page's
  form.
- `network = switchable | clearweb | i2p`: lock rynx to the clearweb or to I2P. Locked to I2P it
  starts in I2P mode and `p` can't leave it.
- Turning history saving on keeps the session's history so far.
- The settings page offers search engines that work without JavaScript (Marginalia, or
  DuckDuckGo's HTML version) or a custom search address, and a Restore defaults button (pressed
  twice, to confirm).
- `A` opens the Internet Archive's latest copy of the page, including pages that failed to load
  (a dead site, a CAPTCHA wall); never for .i2p sites, and never with a password in the address.

### Fixed
- Form fields and choices could be at most 40 columns wide, cutting off addresses on the settings
  page (and wide fields on web pages); they now take up to 80, as space allows.
- New tabs and the first tab had `https://www.rust-lang.org` as their address while showing the
  start page, so Back from the first page visited loaded rust-lang.org. They now show
  `about:help`, and Back returns to the start page.

### Changed
- The default search uses Marginalia's older interface (`old-search.marginalia.nu`), which it
  recommends for text browsers: results start at the top instead of below its filters. Mojeek, a
  possible alternative, now shows JavaScript challenges to text browsers.
- `settings.conf` is written from one description of every setting, so the first-run file and one
  saved from the settings page document the same options.

## [0.2.0-beta.4]

Reader mode, and a filesystem sandbox on macOS.

### Added
- Reader mode (`R`): only an article's text, without menus, sidebars, share buttons, newsletter
  boxes and comments. The main text is the deepest element holding most of the page's prose, so it
  copes with sites that wrap every paragraph in layers of `<div>`s; lines a site sends twice for
  different screen sizes are shown once. It stays on while following links, and pages without an
  article are shown whole with a note.

- A filesystem sandbox on macOS, using Seatbelt (as Safari, Chrome and Firefox do): no reading
  in the home folder except the Downloads and settings folders, and no writing anywhere but those
  and the terminal. HTTPS certificate checks, the clipboard, DNS and download tagging are checked
  to keep working inside it.

### Fixed
- An image sent twice in a row (one copy per screen size, as on Ars Technica) is shown once.

### Changed
- The renderer and the app are split into modules per concern, and page fetching is its own
  pipeline (`app/fetch.rs`); no change in behaviour.
- The README says plainly that rynx is in active beta and that the Linux sandbox is preliminary.

## [0.2.0-beta.3]

rynx is now on crates.io: `cargo install rynx --version 0.2.0-beta.3`.

### Changed
- The program and crate are now called `rynx` (was `rynx_browser`); release archives are named
  `rynx-<version>-<platform>`. The settings folder keeps its name, so settings, bookmarks,
  history and Gemini certificates carry over.
- Licence stated as GPL-3.0-or-later.
- Code formatted with rustfmt, all clippy warnings fixed, and both checked in CI.

## [0.2.0-beta.2]

Second beta: history, the small web (Gemini, Gopher, RSS/Atom), site tools over MCP, Anubis bot
checks passed without JavaScript, and an honest User-Agent.

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
  typed by hand. Opening an MCP server's address as a page says what it is and links to its tools.

- Anubis bot checks are passed without JavaScript: rynx does the check's proof of work natively
  (`fast`/`slow`, `sha256`, `argon2id`, and the no-JavaScript `metarefresh`), fetches the
  stylesheet the `css-load` extension asks for, and loads the guarded page. It's tried once per
  page, gives up after a minute of work, and never follows the check page's honeypot link.
  `anubis = off` turns it off.

### Changed
- More site menus are drawn on one line: lists whose CSS classes lay them out in a row (`flex`,
  `inline`, `lg:flex-row`, as on Ars Technica) count as menus, and a menu repeated for another
  screen size is shown once. Menus of form buttons keep their buttons working, on the menu's
  line, instead of becoming plain text.
- A button's content stays on one line: Ars Technica's view buttons used to split as
  `[ Classic` / `]`.
- The address bar shows the current mode as a clear label (COPY MODE, FIND, EDITING ADDRESS,
  TYPING INTO FORM) with the keys that matter, and Clearweb / I2P mode on the right. Copy mode
  used to show only a dark blue address that was hard to read; it now explains itself and counts
  the characters selected, selected text is black on cyan, and the arrow keys select too.
- rynx now introduces itself honestly as `rynx/<version> (+<project page>)` instead of
  pretending to be Safari. Bot walls like Anubis challenge "Mozilla" browsers with JavaScript
  but let text browsers through: git.kernel.org, for one, now opens directly instead of showing
  a challenge. The `user_agent` setting changes it; I2P mode always sends the generic
  `MYOB/6.66 (AN/ON)`.

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

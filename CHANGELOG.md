# Changelog

## [Unreleased]

### Added
- `scripts/i2p_sites.py` (in the source, Python, nothing to install) finds I2P sites worth a look:
  `update` gathers the address books of reg.i2p, notbob.i2p and stats.i2p through your router,
  `check` visits sites' front pages a few at a time (text only, no redirects followed) to see
  which answer and what they're called, and `search WORDS` and `new` list them with addresses to
  open in rynx. Sites are reached by their `.b32.i2p` addresses, so the router's address book
  isn't changed.

## [0.3.6]

### Added
- Encrypted DNS: the `dns` setting (also on the settings page) looks names up over DNS over
  HTTPS through Quad9, Mullvad, Cloudflare or a provider's own address, instead of your system's
  resolver, so your network and its DNS server can't see or change which sites you look up. It
  covers the web, Gemini and Gopher; I2P is unchanged, since the router looks its names up. There's
  no falling back to the system's resolver, which would show the names after all; names only
  your network knows (`printer.local`, `router.lan`) still go to it. Public names pointing at
  your local network are refused as before. On by default, through Quad9 (a Swiss non-profit
  that keeps no addresses of who asks); `dns = system` goes back to the system's resolver, for
  networks that block encrypted DNS or whose Wi-Fi sign-in page needs their own, as rynx's
  error then says.
- Encrypted Client Hello (ECH), with encrypted DNS: a site that publishes a key in DNS gets its
  name encrypted as the connection starts, where it otherwise shows to anyone on the way
  (Cloudflare's sites do, for one). A site refusing its key is reached as before. rustls does the
  TLS; its HPKE encryption comes from the pure-Rust `hpke` crate, checked against RFC 9180's
  test vectors, so the builds still need no C crypto library.
- Pages that say "you need JavaScript to continue" often carry their own way on for browsers
  without it, which rynx now takes:
  - A meta refresh (`<meta http-equiv="refresh">`, often inside `<noscript>`) of up to 5 seconds
    is followed like a redirect: checked the same way (never from a public site into your local
    network), and left out of history, so Back returns past it. Pages sending rynx round in
    circles are stopped after 5. A slower one is shown above the page as a link.
  - A page that's only a script sending you elsewhere (`location = "…"`, `location.replace(…)`)
    shows where it would go, as a link at the top. The script isn't run: the address is read
    from its text, and only a single fixed address counts.

### Changed
- rynx introduces itself to websites as Safari on a Mac again, the identity millions of visitors
  send, instead of as `rynx/<version>`, which marked every visit as a rynx user's. Bot walls like
  Anubis check it more often, and rynx passes their checks itself. With it go Safari's other
  headers (what it accepts for pages and images, its language, its compression), so a request
  doesn't give itself away; the TLS handshake still differs from Safari's. The `user_agent`
  setting still changes it (without Safari's headers), and I2P mode still sends the generic
  `MYOB/6.66 (AN/ON)` and nothing more.
- Images in the page are sized as the page sizes them: an image's share of the page's width
  (from its width and height attributes, or its own size) is its share of the terminal's, so a
  gallery's thumbnails stay small and a page's main picture is large, where they used to be drawn
  at their pixel size (often tiny). No image is taller than the screen. `+` and `-` draw them
  larger or smaller, for the session.
- Galleries (Wikipedia's, WordPress's and others marked as galleries) lay their images out side
  by side with their captions under them, in rows across the width. A caption used to end the
  row, so Wikipedia's galleries came out one image under another.
- The full-screen viewer (`i`) is for reading: a tall image (a comic page) fits the width and
  scrolls (`j`/`k`, Space, `g`/`G`), `+`/`-` zoom (`h`/`l` move across), `0` fits the width, and
  `n`/`p` go to the page's next or previous image, with the next one loaded ahead. Space at the
  bottom of an image goes on to the next.
- Images up to 32 MB load (8 MB before); long comic strips decode, and are kept sharper.

### Fixed
- The page cache could track you like a cookie. To ask whether a cached page had changed, rynx
  sent the site back the `ETag` (or `Last-Modified` date) it gave with the page, and a site can
  make that unique to each visitor and recognise them by it, across restarts with the disk cache
  on, even when it isn't allowed cookies. Now only sites you let keep cookies (`C`) are asked;
  others' pages are downloaded again once stale, and their validators are never kept.

## [0.3.5]

### Fixed
- I2P pages often failed with "connection closed before message completed", until reloaded.
  rynx reused its connection to the router's proxy for the next request, and the router closes
  one when the I2P stream behind it ends, often just as that request goes out. Each I2P request
  now has a connection of its own (the router is on your computer, so this costs nothing).

## [0.3.4]

### Fixed
- Text pasted into the terminal (`Cmd+V`, `Ctrl+Shift+V`) arrived as typed keys: a line break in
  it was Enter, and the text after it ran as commands (a "t" opened a tab, a "q" quit). rynx now
  turns on bracketed paste, so a paste arrives as one piece: on one line in the address bar or
  find, into the form field being typed in, and on a page not at all.
- Images in tables (forms laid out as tables, like imageboards' captchas, infoboxes, galleries)
  stayed placeholders even once loaded. They're drawn in their cells now, and their columns make
  room for them; an image wider than its column is drawn smaller, keeping its shape.

### Added
- NomadNet pages on the Reticulum network: `nomadnet://<node>/<path>` addresses (or NomadNet's own
  `<node>:/path`, or a node's address alone), fetched by a small bridge program on your computer
  (`reticulum-bridge` in the downloads, or `bridges/reticulum` in the source; it needs Reticulum's
  `rns` from pip) and shown as NomadNet shows them. Files on nodes download with `d`; a node
  that can't be reached shows a saved copy if rynx has one. `reticulum_bridge` in settings says
  where the bridge listens. Locked to I2P only, rynx doesn't open them: Reticulum isn't I2P.
- Micron, NomadNet's page markup, drawn as NomadNet draws it: sections and their indents,
  headings (as anchors), colours, bold, italic and underline, centred and right-aligned lines,
  dividers, tables, literal blocks, links and fields (shown; sending them comes later). Any page
  ending in `.mu` is shown as Micron, whatever type a web server sends it as.

## [0.3.3]

### Changed
- The settings page makes the I2P lock clearer: the network choice has a label ("Clearweb or I2P")
  and options that say what they do ("I2P only: p can't leave it"), and the I2P router section
  says that starting a router doesn't keep rynx in I2P. Turning I2P mode on with `p` mentions the
  lock.

### Added
- Anubis's `hashx` check is passed without JavaScript, like its other methods: HashX (the Tor
  Project's `hashx` crate, as Anubis uses, here without its JIT compiler) seeded with the
  challenge, as Anubis's own client does.
- `about:i2p`: the router's console, and how to get torrents and mail over I2P with the programs
  made for them (Java I2P's I2PSnark and SusiMail, or a SAM torrent client and mail tunnels with
  i2pd, with the lines to add). It's linked from the start page and settings.
- The i2pd rynx starts can let other programs on this computer use it through SAM, at
  `127.0.0.1:7656`: torrent clients (XD, qBittorrent, BiglyBT), pboted for I2P-Bote, and others.
  Off by default; a checkbox in settings, or `i2p_router_sam = on`. If another program has the
  port, SAM stays off and the settings page says so.
- Opening a magnet link copies it, to paste into a torrent client, instead of "Unsupported link
  type".

### Fixed
- Sites listed on reg.i2p, and other names the router didn't know yet, couldn't be opened. i2pd
  adds a name from an address helper link only after you press Continue on its confirmation page,
  and checks that by the Referer, which rynx never sent, so the page came back again and again.
  rynx now sends one, the site's address alone, for that Continue link only. And the jump
  services rynx asks for unknown names were two dead addresses and one working one; it now asks
  the ones i2pd lists (reg.i2p, notbob.i2p, stats.i2p), by their b32 addresses, and shows the
  router's confirmation when one of them leads there.
- In I2P mode, addresses on your own computer, such as the I2P router's console at
  `127.0.0.1:7070`, couldn't be opened: they went to the I2P proxy, which can't reach them. They
  now go directly when typed or opened from bookmarks or history; links to them from sites are
  refused, so an I2P site can't steer you into the router's console. Typing `localhost:8000` or
  `127.0.0.1:7070` without `http://` opens them over HTTP, as local services expect (before,
  `localhost:8000` was searched for).

## [0.3.2]

### Changed
- The start page (`?`) uses a wide screen: its sections flow into two or three columns, so most
  or all of it fits without scrolling, and it stays in one column on narrow screens.
- The top of the window fits narrow screens. The status drops its "Browser - [Status: ...]"
  frame before cutting its words, and ends in "…" when cut; tab titles shorten, and when there
  are too many tabs the current one stays in view with its number ("Tabs 3/8"); the address bar's
  title shortens its labels (the I2P label always stays); and a long address being typed shows
  its end, where the typing is.

## [0.3.1]

### Fixed
- Backspace typed an "h" in the address bar and in find, and didn't go back on a page, in
  terminals that send ^H for it (Ctrl+H, as some openSUSE setups do). Ctrl+H is now Backspace,
  and other Ctrl combinations no longer type their letter there.
- Tables whose cells span columns, like Wikipedia's navigation boxes, made those columns only a
  few characters wide, breaking every word onto its own line. The rule under a table's header
  also crosses only where the next row's cells divide.

### Changed
- A Cloudflare bot check says what it is: it needs JavaScript, and `A` opens the Internet
  Archive's copy instead.

## [0.3.0]

The first release out of beta. rynx reads the web, Gemini, Gopher and feeds with no JavaScript;
draws images in capable terminals; keeps a cache and snapshots; lets you stay signed in to the
sites you choose; uses I2P through a router it can start; and updates itself with signed
releases, all in a sandbox on Linux and macOS. The README lists everything it does.

### Changed
- Keys closer to Vim and Vim-style browsers: `n` / `N` between matches (`>` / `<` still work),
  `o` and `O` (in a new tab) for an address, `t` a new tab, `F` opens the selected link in a new
  tab, `J` / `K` between tabs (with `[` / `]`), `x` closes a tab (with `w`) and `X` reopens it,
  `Y` copies the page's address, and `D` twice clears history (it was `x`). `keys.conf` can
  change any of them back.
- Updates follow stable releases by default; `update_channel = beta` (or the settings page) adds
  test versions.

## [0.2.0-beta.10]

### Fixed
- Text in scripts whose width terminals disagree about (Indic and Myanmar vowel signs and
  conjuncts, emoji sequences, zero-width characters) shifted the rest of its line and left stray
  characters on screen as you scrolled, in Kitty especially (Wikipedia's language menu). Rows
  with such text are now written cell by cell, each at its own position.
- When some of a page's images couldn't be shown, the status line named no reason.
- With an image method chosen in settings, rynx asked any terminal about images at startup, and
  one that didn't answer in full could take the first keys typed. Only terminals known to draw
  images are asked now; a chosen method elsewhere is used without asking.
- rynx asked the system keychain for its cookie key at every start, even if no site had ever been
  allowed cookies, so macOS asked about it (after each update, too) for nothing. The keychain is
  now asked only once a site keeps cookies, and `cookie_encryption = off` (or the settings page)
  skips it altogether.

### Added
- Galleries and grids of loaded images are laid out side by side while they fit, each caption
  under its image, instead of one under another.

## [0.2.0-beta.9]

(Tagged as 0.2.0-beta.8 at first, but that release's builds failed and nothing was published.)

### Fixed
- Gemini and Gopher sites with international names can be reached (their names are now
  encoded for the lookup and the request).
- A `<button>` after a form field was drawn cut short (`[n in]` for `[Sign in]`).
- The cursor was invisible on empty lines (between paragraphs, or below a short page's text).
- After jumping to a section within a page, Tab skipped the first link after it (and after the
  last section, went back to the top of the page).
- Leaving find with Enter left "SEARCH MODE" on the status line; it now says how many matches
  there are.
- With a site unreachable, the status line cut off the note that the page shown is a saved
  copy; the note now comes first, and request errors give their cause before the address.
- An `about:` address rynx doesn't have (`about:blank`) was sent to the search engine.
- On Debian and Ubuntu, the i2pd rynx starts (from the distribution's package) failed at once:
  their AppArmor profile only lets i2pd write under `~/.i2pd`, so its data is now kept in
  `~/.i2pd/rynx-browser` on Linux.

### Added
- Terminal tests (`tests/terminal/`): the real program run in a pseudo-terminal, with keys,
  the mouse and resizing, against local web, Gemini, Gopher, MCP and fake I2P servers, covering
  most of TESTING.md's checks.
- Staying signed in: `C` lets the site you're on keep cookies (before signing in), `C` again
  forgets them, and `about:cookies` lists the sites. A site is everything under its registered
  name; its cookies are taken from and sent to it only, never for a whole public suffix. Lasting
  clearweb cookies are saved in `cookies.json` (readable only by you); I2P sites are allowed
  until rynx quits and their cookies never saved. A page asking for a password says how to stay
  signed in. Cookies carry over when settings change.

### Security
- **I2P mode sent https:// requests straight to the clearweb.** The I2P clients proxied only
  `http://` (reqwest's `Proxy::http`), so `https://` addresses in I2P mode (typed sites,
  searches, the Internet Archive key, tools and images on https pages) bypassed the I2P router
  and revealed your IP address to those sites. Every request in I2P mode now goes through the
  router, `https://` as CONNECT for its outproxy. Present since I2P mode was added; found by
  the new terminal tests' fake I2P proxy.
- Saved cookies, and the list of sites allowed them, are encrypted (ChaCha20-Poly1305) with a
  random key kept in the system keychain: `cookies.enc` replaces the plain files, which are
  encrypted and removed on the first start with a keychain. Without one they're saved as before,
  and `about:cookies` says how they're kept. On Linux the key is asked for after the sandbox, so
  the keychain library's threads are sandboxed too; on macOS before, since the sandbox closes
  the keychain's files.
- Release programs carry their exact dependency list (`cargo auditable`), so a downloaded copy
  can be checked for known vulnerabilities with `cargo audit bin rynx`; the release workflow
  checks each build's list.
- A backup release key is built in alongside the release key, its private half kept offline: if
  the release key is ever lost or exposed, a release signed with the backup key still updates
  every copy (CONTRIBUTING.md has the steps). The release workflow accepts either signature.
- Look-alike site names are flagged: international names are shown readably only when safe;
  ones mixing Latin, Cyrillic or Greek letters, made of Cyrillic or Greek letters that look
  Latin (outside their own endings), or containing slash- or dot-like characters stay encoded,
  with a red warning saying what they'd pass for, in the address bar and link and form previews.
  Invisible direction controls (which can make `report‮fdp.exe` read as `reportexe.pdf`) are
  removed from all text shown.
- The sandbox now also forbids starting programs (Landlock: no Execute anywhere; Seatbelt:
  process-exec and fork denied) and listening for connections (Landlock's TCP bind rule;
  Seatbelt's network-bind), which rynx never does once running. Crash dumps are turned off.
- Public names that point at this computer or the local network are refused (DNS rebinding):
  rynx looks every name up itself, for the web, images, tools, Gemini and Gopher, and connects
  only to the addresses it checked. Names local by their form (`printer.local`, `router.lan`,
  `*.home.arpa`, `*.internal`, single words) are now also treated as local, so public pages
  can't redirect or link the browser to them; `local_names` allows a company network's names.
- Failed requests say why (a refused name, a refused connection), not only "error sending
  request".

## [0.2.0-beta.7]

### Added
- rynx can start the I2P router, i2pd, when it starts (`i2p_router = start`, or the settings
  page), if it's installed and no router is already running: with its own data folder, only the
  HTTP proxy, and no transit traffic unless it's kept running after rynx quits
  (`i2p_router_keep`). Its progress (joining the network, building tunnels, ready) shows on the
  status line and the settings page, read from its local console; a router left running by a
  crash is recognised and stopped. `i2pd_path` gives the program's location.
- Key bindings can be changed in `keys.conf` (written on first run with every action and its
  default keys): add keys, replace them (`only`) or remove them (`none`), with Ctrl, Alt and
  function keys. `about:keys` lists the keys in force and any mistakes in the file; `?` opens the
  start page.
- Development: `scripts/check.sh` runs what CI runs (formatting, lints, tests, the fuzz targets'
  build, `cargo deny`, the oldest supported Rust); `scripts/release.sh <version>` makes a release
  start to finish (version, checks, push, waiting for CI, tag).
- Coverage-guided fuzzing (`fuzz/`, `cargo fuzz`) of pages, Gemini, Gopher, feeds, Anubis pages,
  cookies, settings, keys.conf and the update list, nightly in CI, with each target's corpus kept between
  runs, seed inputs, and replay of past crashes from `fuzz/regressions/`.
- A weekly CI run of the tests that need the internet (skipping the revocation test on macOS
  runners, which often can't reach the revocation servers), and a test coverage report on every CI
  run (failing below 80% of lines).
- CONTRIBUTING.md, SECURITY.md (private vulnerability reports), issue forms and a pull request
  checklist.

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

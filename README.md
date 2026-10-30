# RYNX BROWSER

A private, hardened terminal web browser written in Rust, loosely inspired by **Lynx** and
**w3m**, with **no JavaScript**. It also browses Gemini, Gopher, RSS feeds and I2P.

> **Out of beta since 0.3.0.** rynx is young: please report problems, and
> anything that looks like a security issue (see [SECURITY.md](SECURITY.md)), on
> [GitHub](https://github.com/rry-web/rynx-browser/issues). The filesystem sandbox hasn't yet been
> checked on a wide range of systems.

## Installation

With [Rust](https://rustup.rs) 1.90 or newer:

```sh
cargo install rynx
```

Or download your system's build from the [releases page](https://github.com/rry-web/rynx-browser/releases)
(Linux x86_64 and ARM64, macOS Apple Silicon and Intel, Windows), extract it, and run `rynx` in a
terminal. The builds aren't code-signed yet, so macOS and Windows warn the first time; the release
notes say how to proceed and how to verify a download. Once installed, rynx updates itself (`U`).

From source: clone this repository and run `cargo run --release`. Options: `--version`, `--help`,
and `--no-sandbox` (for troubleshooting).

## Getting started

Press **`e`** (or **`o`**), type an address or search words, and press Enter. **`Tab`** steps through links and
form fields (the bottom border shows where each goes), **Enter** follows a link or starts typing
in a field, **Backspace** goes back, and **`q`** quits. **`?`** shows every key, **`S`** the settings.

## Features

**Reading the web**
- Pages laid out for the terminal: headings, lists, quotes, tables in columns, code, compact site
  menus (`Home · News · Sport`); plain text and JSON as written
- Reader mode (`R`): only an article's text, staying on as you follow links
- Forms: text fields, checkboxes, choices and buttons, sent with GET or POST
- Tabs, Back and Forward to where you were, jumps within a page, find (`/`), copy mode (`v`)
- Images when you want them (`I`): drawn with Kitty, iTerm2 or Sixel graphics, or coloured blocks,
  in the page or full screen (`i`); optionally loading by themselves
- The Internet Archive's copy of a page that's gone or blocked (`A`)
- Bookmarks (`b`, `B`) and history (`s`), kept on disk only if you turn it on

**Beyond the web**
- [Gemini](https://geminiprotocol.net) and Gopher sites, with Gemini certificates trusted on first use
- NomadNet pages on the [Reticulum](https://reticulum.network) mesh (`nomadnet://`), drawn as
  NomadNet draws them, through a small bridge program ([bridges/reticulum](bridges/reticulum))
- RSS and Atom feeds shown as pages; pages offering a feed say so
- Site tools: sites can offer interactive features as [MCP](https://modelcontextprotocol.io)
  tools, shown as forms; nothing is sent until you've seen it and confirmed
- I2P: `.i2p` sites through a local router, which rynx can start for you (i2pd); a setting locks
  rynx to I2P so a stray key can't leave it. `about:i2p` shows how to get torrents and mail through
  the router's own programs, and magnet links are copied for a torrent client. The router rynx
  starts can let torrent clients and other I2P programs on your computer use it (SAM, opt-in)

**Slow connections and staying in control**
- A page cache: Back is instant, unchanged pages aren't downloaded again, unreachable sites show
  the copy kept (on disk too, if you turn it on)
- Dated snapshots of sites you track (`T`, `Z`)
- Downloads that resume where they stopped, tagged so your system checks them
- Staying signed in to the sites you choose (`C`), with their cookies encrypted by a key in the
  system keychain (asked for only once a site keeps cookies; `cookie_encryption = off` skips it)
- Anubis bot checks passed without JavaScript, by every method it uses
- Changeable keys (`keys.conf`), settings in the browser (`S`) or in `settings.conf`
- Signed updates, checked daily and installed when you agree (`U`)

**Light**: about 16 MB of memory, pages laid out in the background so nothing freezes, and only a
page's HTML downloaded (no scripts, styles, fonts or adverts; images when you ask).

## Keys

The defaults; `about:keys` shows the keys in force, and `keys.conf` in the settings folder changes
them (`reload = F5` adds a key, `reload = F5 only` replaces them, `reload = none` removes them).

| Keys | Action |
| :--- | :--- |
| `e` or `o`, `O` | Type an address or search words (`O`: in a new tab) |
| `Tab` / `Shift+Tab`, `Enter`, click | Select links and fields; open or use the selected one |
| `Backspace` / `Right` (or `H` / `L`), `r` | Back / forward, reload |
| `h` `j` `k` `l`, arrows, wheel, `PageUp` / `PageDown`, `Ctrl+u` / `Ctrl+d`, `g` / `G` | Move and scroll |
| `t`, `x` (or `w`), `X` | New tab, close tab, reopen the tab closed last |
| `F` or `Ctrl`+click, `J` / `K` (or `[` / `]`) | Open the link in a new tab; previous / next tab |
| `/`, `n` / `N` (or `>` / `<`) | Find on the page, next / previous match |
| `v` (then `y`) | Copy mode: select text and copy it |
| `R`, `Shift+V` | Reader mode, page source |
| `I`, `i` | Load the page's images, view one full screen |
| `d` | Download the selected link |
| `b`, `B`, `s` | Bookmark the page, show bookmarks, show history (`D` twice there clears it) |
| `Y` | Copy the page's address |
| `C` | Let this site keep cookies (stay signed in), or forget them |
| `T`, `Z`, `A` | Track a site's snapshots, show snapshots, the Internet Archive's copy |
| `p` | Switch I2P mode (unless settings lock it) |
| `S`, `U`, `?`, `q` | Settings, updates, all keys, quit |

While typing in a field: `Enter` sends the form, `Esc` stops typing, `Ctrl+u` clears, `Ctrl+v`
pastes. In the address bar: `Esc` cancels, `Ctrl+y` / `Ctrl+v` copy and paste.

## Settings

`S` shows every setting as a form. They're kept in `settings.conf` in your config folder
(`~/.config/rynx-browser` on Linux, `~/Library/Application Support/rynx-browser` on macOS,
`%APPDATA%\rynx-browser\config` on Windows), which the first run writes with every option
explained and commented out: the home page, the search engine (Marginalia by default, or
DuckDuckGo's HTML version, or any address), locking to the clearweb or I2P, the I2P proxy and
starting i2pd, history, the disk cache, images, updates, the browser's identity, and names on
your own network.

rynx introduces itself honestly as `rynx/<version>`, which bot walls usually let through without
JavaScript; when one shows an Anubis check anyway, rynx does its proof of work itself.

## Security and privacy

- **No JavaScript, no tracking:** no scripts, storage or `Referer`; no cookies except Anubis's
  passes and those of sites you choose to stay signed in to, sent only to them. History and the
  disk cache are off unless you turn them on. In I2P mode rynx sends a generic identity, and every
  request goes through the I2P router, except addresses on your own computer (like the router's
  console at 127.0.0.1:7070), which you type, and which I2P sites can't link to.
- **HTTPS everywhere,** with certificates checked by your operating system (revocation too on
  macOS and Windows) and TLS 1.2 or newer.
- **A sandbox** on Linux (Landlock) and macOS (Seatbelt): rynx can write only to its own folders
  and Downloads, can't read the rest of your home folder, and can't start programs or listen for
  connections. Crash dumps are off.
- **Hostile pages contained:** terminal escape codes stripped, oversized and "bomb" pages refused,
  and nothing reaches your local network from a public page: not links, redirects, forms, images,
  tools, or names whose DNS points there (DNS rebinding).
- **Honest links:** every destination is shown before you follow it; disguised addresses
  (`https://paypal.com@evil.example/`), look-alike names (`аpple.com` with a Cyrillic `а`) and
  text-reversing characters are flagged or removed; `mailto:`, `javascript:` and `file:` links are
  refused.
- **Safe downloads,** tagged as from the internet (macOS quarantine, Windows Mark of the Web,
  Linux origin), never overwriting a file.
- **Signed updates:** a release is installed only if its checksums carry the release key's
  signature (a backup key is kept offline). Release programs carry their dependency list, so
  `cargo audit bin rynx` checks a copy for known vulnerabilities.

## Features to add

The longer list, with notes and ideas, is in [ROADMAP.md](ROADMAP.md).

- **Your own MCP servers:** tools from servers you configure, usable on any page
- **Sign-in for site tools** (OAuth), and servers asking questions while a tool runs
- **Encryption at rest** for history, the cache and snapshots (cookies are encrypted already)
- **Session restore** (opt-in): reopen your tabs
- **SVG images**, and drawing images only partly on screen
- **Downloads from Gemini and Gopher**, and Gemini client certificates
- **Finer cookie control** (per cookie, and third-party rules), and history times in your time zone
- **A Windows sandbox**, and **code-signed** macOS and Windows builds
- **Packages:** Homebrew, the AUR and others
- **Reticulum, directly:** without the bridge; and sending NomadNet pages' fields
- **A built-in I2P router** (the Rust router emissary, once it matures), so I2P needs nothing
  installed; and the started router's progress with older i2pd versions, whose console can be
  slow to answer while starting

Known limits: sites that need JavaScript won't work, including those behind Cloudflare's bot
check (the Internet Archive's copy or reader mode sometimes help); macOS Terminal draws images only as coloured blocks; a proxy set in the
system's settings bypasses the local-network check for names.

## Development

```sh
scripts/check.sh --quick          # formatting, lints and tests
scripts/check.sh                  # everything CI checks
scripts/release.sh 0.3.3   # a release, start to finish
```

CI tests every push on Linux, macOS and Windows, including terminal tests that run the real
program against local servers; fuzzing runs nightly, live-site tests weekly and mutation testing
monthly. [CONTRIBUTING.md](CONTRIBUTING.md) covers building, testing, conventions and releases;
[TESTING.md](TESTING.md) maps every check to its test and lists the few that need a person;
[CHANGELOG.md](CHANGELOG.md) lists the changes.

## License

rynx is free software, licensed under the GNU General Public License, version 3 or (at your
option) any later version (GPL-3.0-or-later). See [LICENSE.txt](LICENSE.txt).

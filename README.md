# RYNX BROWSER

A terminal web browser written in Rust, loosely inspired by **Lynx** and **w3m**, with absolutely
**zero JavaScript**.

> **Status: beta.** Version 0.2.0-beta.1 is the first release with pre-built binaries. Please report
> problems as [GitHub issues](https://github.com/rry-web/rynx-browser/issues).

- **Readable pages:** headings, numbered and nested lists, blockquotes, aligned table columns, and
  preformatted code, laid out for the terminal. Site menus are shown compactly on one line
  (`Home · News · Sport`) instead of a long column of links. Plain-text files (`.txt`, JSON,
  source code) are shown as text with clickable URLs; images, PDFs and other files are offered for
  download.
- **Forms:** search boxes and other forms work: text fields, checkboxes, choices and buttons, sent
  with GET or POST.
- **Tabs, history and search:** Back and Forward return you to where you were on each page,
  in-page links jump without reloading, and `/` finds text on the page.
- **Fast:** pages are laid out on a background thread, so even huge or slow pages never freeze the
  screen.
- **Private and hardened:** no cookies, no JavaScript, HTTPS everywhere, and on Linux a filesystem
  sandbox. See [Security and privacy](#security-and-privacy).
- **The small web:** [Gemini](https://geminiprotocol.net) and Gopher sites, and RSS/Atom feeds
  shown as readable pages.
- **Site tools instead of JavaScript:** sites can offer interactive features as
  [MCP](https://modelcontextprotocol.io) tools, which rynx shows as forms. Nothing runs on your
  computer, and nothing is sent until you've seen exactly what it is and confirmed.
- **I2P:** browse `.i2p` sites through a local I2P router.

## Installation

### Pre-built binaries

Download the archive for your system from the
[releases page](https://github.com/rry-web/rynx-browser/releases) (beta builds are marked
*Pre-release*), extract it, and run `rynx_browser` in a terminal:

| System | Archive name contains |
| :--- | :--- |
| Linux x86_64 | `x86_64-unknown-linux-gnu`, or `x86_64-unknown-linux-musl` for any distribution |
| Linux ARM64 | `aarch64-unknown-linux-gnu` |
| macOS (Apple Silicon / Intel) | `aarch64-apple-darwin` / `x86_64-apple-darwin` |
| Windows | `x86_64-pc-windows-msvc` |

The binaries aren't code-signed yet, so macOS and Windows warn the first time; the release notes
explain how to proceed and how to verify the download (`SHA256SUMS` and a build provenance
attestation).

### From source

Install [Rust](https://rustup.rs) and run:

```sh
cargo run --release
```

Command-line options:

| Option | Effect |
| :--- | :--- |
| `--version`, `-V` | Print the version |
| `--help`, `-h` | Print usage |
| `--no-sandbox` | Turn off the Linux filesystem sandbox (for troubleshooting) |

## Using it

Press **`e`**, type an address (or search words) and press Enter. Words that aren't an address are
searched with [Marginalia](https://marginalia-search.com), a search engine for the
non-commercial web. Marginalia sometimes shows a "wait a moment" page to slow down bots; select its
"proceed manually" link to continue, or set a different search engine in [settings](#settings).

The address bar's title shows what mode you're in: **COPY MODE** (after `v`, with how many
characters are selected), **FIND**, **EDITING ADDRESS** or **TYPING INTO FORM**, with the keys
that matter, and on the right whether you're on the clearweb or in I2P mode.

Press **`Tab`** to step through the links and form fields on a page. The bottom border always shows
where the selected link goes, or what a form field is and where its form sends what you type.

To use a form, select a field and press **Enter** to start typing, then **Enter** again to submit
(or **Esc** to stop typing). There are no cookies, so sites that need you to log in won't keep
you signed in.

Type `gemini://` or `gopher://` addresses as you would a web address. Gemini input prompts and
Gopher searches appear as a form with one text field. Opening an RSS or Atom feed shows its posts
as a page, and pages that announce a feed show a `Feeds:` link above the page. Gemini and Gopher
aren't available in I2P mode, and downloading from them isn't supported yet.

### Site tools (MCP)

A page that offers tools shows a `Tools:` link at the top. It opens a page listing each tool with
a form for its input. Fill it in and press Enter: a box shows exactly what will be sent, and
**`y`** runs the tool while **`n`** cancels. The result opens as a page; Back returns to the tools.
You can also type an MCP server's address, such as `https://mcp.deepwiki.com/mcp`: rynx
recognises it and offers a link to its tools (or type `about:tools?server=<address>` to go straight
there).

For site authors: announce an MCP server (Streamable HTTP transport) in your page's `<head>`
with `<link rel="mcp" href="/mcp">`. It must be on the same site as the page (or a subdomain).
Tool titles, descriptions, input schemas (strings, numbers, booleans, choices; arrays and
objects are typed as JSON) and the `readOnlyHint` / `destructiveHint` annotations are shown.
Results can contain text, links and text resources. Sign-in (OAuth), server requests to the
user, and the older HTTP+SSE transport aren't supported yet.

Downloads (**`d`** on a link) are saved to your Downloads folder after you confirm.

Press **`b`** to bookmark a page (again to remove it) and **`B`** to see your bookmarks. Press
**`s`** to see the pages you've visited. History is kept only until you quit, unless you turn on
`history = on` in [settings](#settings).

## Settings

The first run creates `settings.conf` in your config folder (`~/.config/rynx-browser` on Linux,
`~/Library/Application Support/rynx-browser` on macOS, `%APPDATA%\rynx-browser\config` on Windows),
with every option commented out. Remove the `#` to use one:

```ini
home_page = https://en.wikipedia.org/
search_url = https://html.duckduckgo.com/html/?q={query}
i2p_proxy = http://127.0.0.1:4444
download_dir = /path/to/folder
history = on
user_agent = Mozilla/5.0 (X11; Linux x86_64) Firefox/140.0
anubis = off
```

rynx introduces itself honestly to websites, as `rynx/<version> (+<project page>)`. Bot walls
such as [Anubis](https://anubis.techaro.lol) demand JavaScript from browsers claiming to be
"Mozilla", but usually let text browsers through, so this gets you past them without running
any scripts. A few sites refuse browsers they don't recognise; `user_agent` lets you send another
browser's identity instead (at the cost of meeting those bot walls again).

When a site does show an Anubis check ("Making sure you're not a bot"), rynx passes it without
JavaScript: it does the same proof of work the check's script would (finding a hash with enough
leading zeros, using every CPU core), hands in the answer, and loads the page you asked for. The
status line says when it's doing this. The `fast`, `slow`, `sha256`, `argon2id` and
`metarefresh` methods are supported; `hashx` isn't yet. `anubis = off` shows the check's page
instead.

Bookmarks are kept next to it in `bookmarks.txt`, history (when `history = on`) in
`history.txt`, and the certificates of Gemini sites you've visited in `gemini_known_hosts.txt`. Pages visited in I2P mode are never saved to the history file.

## Key bindings

The same list is on the start page.

### Moving around
| Key | Action |
| :--- | :--- |
| **`h` `j` `k` `l`** | Move the cursor (vim-style); the view follows |
| **`Up` / `Down`**, scroll wheel | Scroll; the cursor stays on screen |
| **`PageUp` / `PageDown`**, **`Ctrl+b` / `Ctrl+f`** | Scroll a screen |
| **`Ctrl+u` / `Ctrl+d`** | Scroll half a screen |
| **`g` / `G`**, **`Home` / `End`** | Top / bottom of the page |

### Links and history
| Key | Action |
| :--- | :--- |
| **`Tab` / `Shift+Tab`** | Select the next / previous link or form field |
| **`Enter`**, click | Open the selected link, or use the form field |
| **`Backspace`**, **`Left`**, **`H`** | Back, to where you were on the page |
| **`Right`**, **`L`** | Forward |
| **`r`** | Reload, keeping your place |
| **`b`** / **`B`** | Bookmark the page (or remove it) / show bookmarks |
| **`s`** | Show the pages you've visited (there, **`x`** twice clears them) |
| **`d`** | Download the selected link |
| **`Esc`** | Clear a finished or failed download |

### Forms
| Key | Action |
| :--- | :--- |
| **`Enter`** on a text field | Start typing |
| **`Enter`** while typing | Submit the form (in a text area: new line) |
| **`Esc`** while typing | Stop typing, keeping the text |
| **`Ctrl+u`** / **`Ctrl+v`** while typing | Clear the field / paste |
| **`Enter`** on a checkbox, option or choice | Tick it, pick it, or show the next choice |
| **`Enter`** on a button | Submit the form |

### Tabs
| Key | Action |
| :--- | :--- |
| **`n`** / **`w`** | New tab / close tab |
| **`t`**, **`Ctrl`+click** | Open the selected link in a new tab |
| **`[`** / **`]`** | Previous / next tab |

### Address bar (press `e`)
| Key | Action |
| :--- | :--- |
| **`Enter`** | Go (or search, if it isn't an address) |
| **`Esc`** | Cancel and restore the address |
| **`Ctrl+u`** / **`Ctrl+k`** | Clear / clear and paste |
| **`Ctrl+y`** / **`Ctrl+v`** | Copy / paste |

### Finding text, copying, and more
| Key | Action |
| :--- | :--- |
| **`/`** | Find on the page (live highlighting) |
| **`>`** / **`<`** | Next / previous match |
| **`v`** | Copy mode: select text with `h` `j` `k` `l` or the arrow keys, **`y`** copies it, **`Esc`** cancels |
| **`Shift+V`** | Show the page source |
| **`p`** | Toggle I2P mode (through the proxy at `127.0.0.1:4444`) |
| **`q`** | Quit |

## Security and privacy

A text browser without JavaScript already avoids the biggest source of browser exploits. On top of
that:

- **No tracking:** no cookies, no storage, no `Referer` header, no JavaScript. The one exception
  is the pass an Anubis check gives you: only Anubis's own cookies are kept, in memory, sent
  only to the site that set them, and forgotten when you quit (with separate ones for I2P). In I2P mode the
  browser always identifies itself as `MYOB/6.66 (AN/ON)`, the generic name the I2P proxy uses, so
  its visits don't stand out (even over HTTPS, where the proxy can't rewrite it). History is saved
  only if you turn it on, and never for I2P sites.
- **HTTPS everywhere:** addresses, redirects, downloads and form submissions are upgraded to HTTPS
  (except `localhost` and `.i2p` sites). Certificates are checked by your operating system,
  including revocation on macOS and Windows, with TLS 1.2 or newer.
- **Linux sandbox:** the browser restricts itself with Landlock (kernel 5.13+): it can write only
  to your Downloads folder and its settings folder, and can't read the rest of your home directory, so a bug can't be used
  to read SSH keys or plant files. The status line shows whether it's active.
- **Hostile pages are contained:** escape sequences that could take over your terminal are
  stripped; oversized, deeply nested or compressed "bomb" pages can't freeze or crash the browser;
  public pages can't redirect into your local network.
- **Careful with links and forms:** the destination of every link and form is shown before you
  use it. `mailto:`, `javascript:` and `file:` links are refused, and so are links and forms that
  disguise their real site with a user name (`https://paypal.com@evil.example/`). A form is never
  allowed to put a password in a web address, and a public page's form can't send data to your
  router or other devices on your local network. Passwords typed into the address bar are hidden
  on screen and never saved in bookmarks.
- **Safe downloads:** files are tagged as coming from the internet (macOS quarantine, Windows Mark
  of the Web, Linux `user.xdg.origin.url`) so your OS checks them before they run; filenames are
  made safe on every OS, and existing files are never overwritten silently.
- **Gemini and Gopher:** Gemini sites usually have self-signed certificates, so the first one
  seen for each site is remembered, and a different one later is refused unless it's signed by a
  certificate authority. Gopher is unencrypted, like plain HTTP. A public page can't link to Gopher
  or Gemini on your local network (where a crafted Gopher link could send commands to services
  like Redis), and Gopher addresses containing line breaks are refused. Feeds are read without
  expanding XML entities, so entity bombs can't exhaust memory.
- **Site tools:** a page can only offer tools from its own site, over HTTPS, and never from
  your local network. Every tool call shows what will be sent and needs **`y`** to go ahead (the
  mouse can't confirm it), and web pages can't submit forms to tools or open results directly.
  Tool servers can't redirect. No AI model is involved, so instructions hidden in tool
  descriptions have nothing to act on; they're just text.
- **I2P:** in I2P mode all traffic goes through the I2P proxy, and `.i2p` addresses are never looked
  up outside it.
- **Maintained dependencies:** `cargo audit` runs in CI on every change and weekly.

## Development

```sh
cargo test                                   # unit, integration and randomised renderer tests
cargo test --test live_tls -- --ignored      # TLS checks against public test servers
cargo test --test smallweb --test feeds --test mcp -- --ignored   # real Gemini, Gopher, feed and MCP servers
FUZZ_ITERATIONS=100000 cargo test --release --test fuzz_render
```

[TESTING.md](TESTING.md) has a checklist of hands-on checks (terminals, touchpads, platforms, I2P)
with results recorded in [TEST_LOG.md](TEST_LOG.md). Changes are listed in
[CHANGELOG.md](CHANGELOG.md).

CI tests every push on Linux, macOS and Windows, fuzzes the renderer and audits dependencies. To
make a release, set the version in `Cargo.toml`, add its section to `CHANGELOG.md`, and push a
matching tag (`git tag v0.2.0-beta.1 && git push origin v0.2.0-beta.1`): the release workflow
builds every platform and publishes the release. Tags containing `-` become pre-releases.

## Roadmap
- [x] **I2P integration** through a local proxy
- [x] **Downloads** with progress, safe filenames and OS security tagging
- [x] **Clipboard** support for the address bar and page text
- [x] **Find on page** with highlighting
- [x] **Rendering** of tables, lists, quotes, code and plain-text files
- [x] **Forms** for searching and other text input
- [ ] **Images** with `ratatui-image` (Sixel/Kitty)
- [x] **Bookmarks**
- [x] **Settings file** (home page, search engine, I2P proxy, downloads folder)
- [x] **Persistent history** (opt-in)
- [x] **Gopher, Gemini and RSS** support
- [x] **MCP tools** offered by sites, as an alternative to JavaScript
- [ ] **Your own MCP servers**: tools from servers you configure, usable on any page
- [ ] **MCP sign-in** (OAuth) for tools that need an account

If you want to contribute, open an issue or pull request on GitHub.

## License

This project is licensed under GPLv3.

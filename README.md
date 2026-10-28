# RYNX BROWSER

A terminal web browser written in Rust, loosely inspired by **Lynx** and **w3m**, with absolutely
**zero JavaScript**.

> **Status: beta.** Version 0.2.0-beta.7 is the latest release. rynx is under active development:
> security protections and fixes keep arriving as issues are found, and some protections (the
> filesystem sandbox on Linux and macOS in particular) are preliminary and haven't yet been checked on a wide
> range of systems. Please report problems, and anything that looks like a security issue, as
> [GitHub issues](https://github.com/rry-web/rynx-browser/issues).

- **Readable pages:** headings, numbered and nested lists, blockquotes, aligned table columns, and
  preformatted code, laid out for the terminal. Site menus are shown compactly on one line
  (`Home · News · Sport`) instead of a long column of links. Plain-text files (`.txt`, JSON,
  source code) are shown as text with clickable URLs; images, PDFs and other files are offered for
  download.
- **Forms:** search boxes and other forms work: text fields, checkboxes, choices and buttons, sent
  with GET or POST.
- **Reader mode:** press **`R`** to see only an article's text, without menus, sidebars, share
  buttons and comments.
- **Tabs, history and search:** Back and Forward return you to where you were on each page,
  in-page links jump without reloading, and `/` finds text on the page.
- **Images, when you want them:** placeholders by default; `I` loads a page's images, drawn in the
  page with Kitty, iTerm2 or Sixel graphics (or coloured text blocks), and `i` shows one full screen.
- **Made for slow connections:** a page cache (Back is instant, unchanged pages aren't downloaded
  again, unreachable sites show the copy kept), dated snapshots of sites you track, and downloads
  that resume where they stopped. Useful on any connection, a big help over I2P.
- **Fast and light:** pages are laid out on a background thread, so even huge or slow pages never
  freeze the screen, and rynx uses about 16 MB of memory. See [Efficiency](#efficiency).
- **Private and hardened:** no cookies (except for sites you choose to stay signed in to), no JavaScript, HTTPS everywhere, and on Linux and macOS a
  filesystem sandbox. See [Security and privacy](#security-and-privacy).
- **The small web:** [Gemini](https://geminiprotocol.net) and Gopher sites, and RSS/Atom feeds
  shown as readable pages.
- **Site tools instead of JavaScript:** sites can offer interactive features as
  [MCP](https://modelcontextprotocol.io) tools, which rynx shows as forms. Nothing runs on your
  computer, and nothing is sent until you've seen exactly what it is and confirmed.
- **I2P:** browse `.i2p` sites through a local I2P router, which rynx can start for you (i2pd),
  and lock rynx to I2P so a stray key can't send a page outside it.
- **Settings in the browser:** `S` opens every setting as a form; `A` opens the Internet Archive's
  copy of a page that's gone or blocked.

## Installation

### With Cargo

With [Rust](https://rustup.rs) 1.90 or newer installed:

```sh
cargo install rynx --version 0.2.0-beta.7
```

Then run `rynx`. (Cargo skips beta versions unless you name one with `--version`.)

### Pre-built binaries

Download the archive for your system from the
[releases page](https://github.com/rry-web/rynx-browser/releases) (beta builds are marked
*Pre-release*), extract it, and run `rynx` in a terminal:

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

Install [Rust](https://rustup.rs), clone this repository, and run:

```sh
cargo run --release
```

Command-line options:

| Option | Effect |
| :--- | :--- |
| `--version`, `-V` | Print the version |
| `--help`, `-h` | Print usage |
| `--no-sandbox` | Turn off the filesystem sandbox (for troubleshooting) |

## Using it

Press **`e`**, type an address (or search words) and press Enter. Words that aren't an address are
searched with [Marginalia](https://marginalia-search.com), an independent search engine for the
non-commercial web, through its simpler interface meant for text browsers. On the settings page
(**`S`**) you can switch to DuckDuckGo's HTML version for the wider web, or enter any search
address.

Press **`R`** for reader mode, which shows only an article's text: its headline, byline,
paragraphs, pictures' descriptions and tables, without the site's menus, sidebars, share buttons,
newsletter boxes and comments. It stays on as you follow links (handy for a series of chapters),
and on pages with no article, such as a home page, the whole page is shown with a note saying so.

The address bar's title shows what mode you're in: **COPY MODE** (after `v`, with how many
characters are selected), **FIND**, **EDITING ADDRESS**, **TYPING INTO FORM** or **READER**, with the keys
that matter, and on the right whether you're on the clearweb or in I2P mode.

Press **`Tab`** to step through the links and form fields on a page. The bottom border always shows
where the selected link goes, or what a form field is and where its form sends what you type.

To use a form, select a field and press **Enter** to start typing, then **Enter** again to submit
(or **Esc** to stop typing).

There are no cookies, so sites forget you between pages, unless you let one keep them: press
**`C`** on a site's page before signing in, and you stay signed in (also after restarting rynx, if
the site's "remember me" cookie says so). **`C`** again forgets its cookies, which signs you out;
`about:cookies` lists the sites you've allowed. A page asking for a password says this on the
status line. A site means everything under its registered name (`accounts.example.com` and
`www.example.com` are both `example.com`), and its cookies are sent only to it. Cookies are saved
in `cookies.json` in the settings folder, readable only by you; I2P sites are allowed until rynx
quits, and their cookies are never saved.

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

### Images

Images show as placeholders (`[A photo of the bridge]`) until you ask for them: **`I`** loads the
page's images, and **`i`** opens the selected one (or the first on screen) full screen. On the
settings page you can let images load by themselves, either only those from the page's own site
(images from elsewhere are often trackers) or all of them, and choose whether loaded images are
shown in the page or only full screen.

How they look depends on the terminal. Kitty, iTerm2, Ghostty and WezTerm show real images; rynx
recognises them and picks the best way to draw. Other terminals, including macOS Terminal, get a
rough picture made of coloured text blocks. The settings page can force a method if detection
gets it wrong. Images are fetched like pages, through the cache, and a public page can never load
images from your local network. SVG images aren't supported yet.

### Updates

rynx checks GitHub for a new version once a day (never in I2P mode) and says so in the status line.
**`U`** shows what's new, and, if you agree, downloads it; it's installed the next time rynx
starts. Before installing, rynx checks the release's signature against the release key built into
it, and the download against the signed checksum, so a tampered release or download is refused.
Copies installed with `cargo install` or Homebrew are updated with those tools; the update page
gives the command. The settings page turns daily checks off or limits updates to stable releases.

### The cache and snapshots

Pages you visit are kept in memory until you quit, so **Back** and **Forward** show them
instantly, and over slow I2P connections that matters a lot. rynx follows each site's caching
rules: a page the site says is still fresh is shown without asking, an older one is checked with
the site ("has this changed?") instead of downloaded again, and pages marked not to be stored
never are. **`r`** always checks with the site. If a site can't be reached (or the I2P router
reports it unreachable), the copy kept is shown, saying when it was saved.

On the settings page you can also keep pages **on disk**, gzip-compressed in rynx's cache
folder with a size limit, so they last between sessions, with a separate switch for I2P pages.
Both are off by default, since a file per page visited is a record of where you've been.

To keep dated **snapshots** of a site, press **`T`** on one of its pages to track it (again to
stop). From then on, each visit to one of its pages saves a snapshot whenever the page changed.
**`Z`** lists the current page's snapshots (or every tracked site's): open any of them to see the
page as it was, and press `r` there for the live page. Snapshots are kept in rynx's data folder
until you delete them from the snapshots page.

Press **`A`** to open the [Internet Archive](https://web.archive.org)'s latest copy of a page:
handy when a site is down, gone, or behind a CAPTCHA or a JavaScript wall. It works on a page that
failed to load, too. The Archive learns which address you looked up.

Downloads (**`d`** on a link) are saved to your Downloads folder after you confirm. If the connection breaks,
a download resumes where it stopped; if it still fails, downloading the same link again later
picks up from what was saved.

Press **`b`** to bookmark a page (again to remove it) and **`B`** to see your bookmarks. Press
**`s`** to see the pages you've visited. History is kept only until you quit, unless you turn on
`history = on` in [settings](#settings).

## Settings

Press **`S`** for the settings page: every setting as a form, applied as soon as you save (except
the downloads folder, which changes when rynx next starts), with links to your history and
bookmarks. The **Network** choice locks rynx to the clearweb or to I2P: locked to I2P, it starts
in I2P mode and `p` can't leave it, so a stray key press can't send a page outside I2P.

Settings are kept in `settings.conf`. The first run creates it in your config folder (`~/.config/rynx-browser` on Linux,
`~/Library/Application Support/rynx-browser` on macOS, `%APPDATA%\rynx-browser\config` on Windows),
with every option commented out. Remove the `#` to use one:

```ini
home_page = https://en.wikipedia.org/
network = i2p
search_url = https://html.duckduckgo.com/html/?q={query}
i2p_proxy = http://127.0.0.1:4444
download_dir = /path/to/folder
history = on
user_agent = Mozilla/5.0 (X11; Linux x86_64) Firefox/140.0
anubis = off
cache = on
cache_i2p = on
cache_size = 500
images = site
image_display = inline
updates = daily
update_channel = beta
local_names = intranet.example.com
i2p_router = start
i2p_router_keep = on
i2pd_path = /opt/i2pd/i2pd
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

### Starting the I2P router

rynx can start [i2pd](https://i2pd.website/) for you, so I2P works without setting a router up:
install it (`brew install i2pd`, `apt install i2pd`, or from its site), then on the settings
page tick "Start i2pd when rynx starts" (or set `i2p_router = start`) and restart rynx.

- **When:** at startup, if no router is already running at the I2P proxy's address (one is left
  alone) and rynx isn't set to always use the clearweb. It's started before the sandbox, so it
  isn't confined by it.
- **How:** with its own data folder (`i2pd` next to the snapshots folder) and empty `i2pd.conf`
  and `tunnels.conf` there for options of your own; only the HTTP proxy rynx uses, with SOCKS,
  SAM and the other client ports off; its web console on `127.0.0.1:7070` (if free), which rynx
  reads to show its progress on the status line and the settings page.
- **Joining the network** takes a minute or so; the first time, names like `stats.i2p` work only
  once its address book has downloaded, about 5 to 10 minutes after that. Until then, use a jump
  service, or a `.b32.i2p` address.
- **When rynx quits** the router stops, and while it runs it carries no one else's traffic
  (transit tunnels), since dropping them at every quit is hard on the network.
  `i2p_router_keep = on` keeps it running instead, ready sooner next time, carrying others'
  traffic as routers normally do; stop it yourself when you want (its process is shown on the
  settings page). A router left running by a crash is recognised and stopped next time.
- `i2pd_path` gives the program's location if it isn't on `PATH` or where package managers put it.

Bookmarks are kept next to it in `bookmarks.txt`, history (when `history = on`) in
`history.txt`, and the certificates of Gemini sites you've visited in `gemini_known_hosts.txt`. Pages visited in I2P mode are never saved to the history file.

## Key bindings

These are the defaults; the start page lists them too (`?`), and `about:keys` shows the keys in
force. To change them, edit `keys.conf` in the settings folder (the first run writes it, with
every action and its keys commented out):

```
reload = F5              # adds F5 to reload's keys
quit = Ctrl+q only       # makes Ctrl+q the only way to quit
bookmark = none          # takes b away
next_tab = Alt+l, Ctrl+n
```

Keys are written as a character (`r`, `R`, `/`), `Ctrl+x`, `Alt+x`, `F1`-`F24`, or a name
(`Enter`, `Esc`, `Tab`, `Shift+Tab`, `Backspace`, `Delete`, `Insert`, arrows, `Home`, `End`,
`PageUp`, `PageDown`, `Space`). Typing in fields, the address bar and find, and copy mode, keep
their own keys. Mistakes are named on the status line and on `about:keys`, and the rest of the
file still applies.

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
| **`U`** | Updates: what's new, and installing it |
| **`I`** | Load this page's images |
| **`i`** | View the selected image (or the first on screen) full screen; `Esc` closes |
| **`T`** | Track this site: keep a dated snapshot whenever one of its pages changes (again to stop) |
| **`Z`** | Snapshots of this page (or of every tracked site) |
| **`A`** | Open the Internet Archive's latest copy of the page (also for pages that failed to load) |
| **`R`** | Reader mode: only the page's main text (stays on as you follow links; `R` again for the whole page) |
| **`Shift+V`** | Show the page source |
| **`p`** | Switch I2P mode on or off (through the proxy at `127.0.0.1:4444`), unless settings lock it |
| **`S`** | Settings |
| **`C`** | Let this site keep cookies, so you stay signed in (again to forget them); `about:cookies` lists them |
| **`?`** | The start page, with these keys |
| **`q`** | Quit |

## Security and privacy

A text browser without JavaScript already avoids the biggest source of browser exploits. On top of
that:

- **No tracking:** no cookies, no storage, no `Referer` header, no JavaScript. The exceptions
  are the pass an Anubis check gives you (only Anubis's own cookies, kept in memory, sent only to
  the site that set them, and forgotten when you quit) and the cookies of sites you choose to stay
  signed in to (`C`), which are only taken from and sent to those sites. A site can't set a cookie
  for a whole public suffix (`co.uk`, `github.io`), and clearweb and I2P cookies are kept apart. In I2P mode the
  browser always identifies itself as `MYOB/6.66 (AN/ON)`, the generic name the I2P proxy uses, so
  its visits don't stand out (even over HTTPS, where the proxy can't rewrite it). History is saved
  only if you turn it on, and never for I2P sites. Pages are kept on disk only if you turn the disk
  cache on (I2P pages only with a separate switch), and snapshots only for sites you track.
- **HTTPS everywhere:** addresses, redirects, downloads and form submissions are upgraded to HTTPS
  (except `localhost` and `.i2p` sites). Certificates are checked by your operating system,
  including revocation on macOS and Windows, with TLS 1.2 or newer.
- **Filesystem sandbox (preliminary):** on Linux (Landlock, kernel 5.13+) and macOS (Seatbelt, the
  sandbox Safari, Chrome and Firefox use), the browser restricts itself at startup: it can write
  only to your Downloads folder and its settings folder, and can't read the rest of your home
  folder, so a bug can't be used to read SSH keys or documents, or plant files. It also can't
  start programs or listen for connections once running (it never needs to), so a bug can't run a
  shell or open a port, and crash dumps (which would hold pages and cookies) are off. The status
  line shows whether it's active.
  It hasn't yet been tested on many Linux distributions and kernels or macOS versions, and
  Windows has no sandbox yet.
- **Hostile pages are contained:** escape sequences that could take over your terminal are
  stripped; oversized, deeply nested or compressed "bomb" pages can't freeze or crash the browser;
  public pages can't redirect into your local network.
- **Names can't reach your local network either:** rynx looks every name up itself and refuses a
  public name that points at this computer or your local network (a site's DNS answering
  `192.168.1.1`, or changing its answer between visits, "DNS rebinding"), for pages, redirects,
  images, tools, Gemini and Gopher alike. The connection goes to exactly the addresses checked.
  Names that are local by their form (`printer.local`, `router.lan`, `nas.home.arpa`,
  `*.internal`, a single word) work when you open them, but public pages can't send you to them.
  Names on a company network that point at internal addresses can be allowed with `local_names`.
- **Careful with links and forms:** the destination of every link and form is shown before you
  use it. `mailto:`, `javascript:` and `file:` links are refused, and so are links and forms that
  disguise their real site with a user name (`https://paypal.com@evil.example/`). A form is never
  allowed to put a password in a web address, and a public page's form can't send data to your
  router or other devices on your local network. Passwords typed into the address bar are hidden
  on screen and never saved in bookmarks.
- **Look-alike names are flagged:** international site names are shown readably (`bücher.de`)
  only when they can't pass for another name. One that mixes Latin, Cyrillic or Greek letters
  (`аpple.com` with a Cyrillic `а`), is made entirely of Cyrillic or Greek letters that look
  Latin under a Latin ending, or contains characters that look like `/` or `.`, stays in its
  encoded form (`xn--pple-43d.com`) with a red warning naming what it would pass for, in the
  address bar and wherever a link's or form's destination is shown. Invisible characters that
  reverse the order of text (which can make `report‮fdp.exe` read as `reportexe.pdf`) are removed
  from everything shown.
- **Safe downloads:** files are tagged as coming from the internet (macOS quarantine, Windows Mark
  of the Web, Linux `user.xdg.origin.url`) so your OS checks them before they run; filenames are
  made safe on every OS, and existing files are never overwritten silently. A resumed download
  checks the file hasn't changed on the server, so two versions are never spliced together.
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
- **Images:** none load until you ask (unless you choose otherwise), since images are how many
  sites track you. Images are decoded by memory-safe decoders with limits on size, so a crafted
  "image bomb" can't exhaust memory, and a public page can never make the browser request an
  image from your local network (a request that can make a router or printer act).
- **I2P:** in I2P mode all traffic goes through the I2P proxy, and `.i2p` addresses are never looked
  up outside it.
- **Signed updates:** releases' checksum lists are signed with an Ed25519 release key kept only as a
  GitHub Actions secret; rynx installs an update only if the signature matches the public key built
  into it and the download matches the signed checksum. Update checks go to GitHub over the
  clearweb, never in I2P mode.
- **Maintained dependencies:** `cargo deny` checks every change, and weekly, for known
  vulnerabilities, licences and where dependencies come from; Dependabot proposes updates. Images
  and every other untrusted format are handled by memory-safe Rust code only.

## Efficiency

Measured on macOS with the release build:

| | |
| :--- | :--- |
| Memory, idle | about 16 MB |
| Laying out a 390 KB Wikipedia article | 0.19 s, 13 MB at most |
| Program size | about 10 MB |

rynx downloads a page's HTML and nothing else: no style sheets, scripts, fonts or adverts, and
images only when you ask. That's often a few percent of what a graphical browser fetches for the
same page. Pages arrive compressed (gzip, Brotli), HTTP/2 reuses connections, Back is served from
memory, a page the site says is unchanged costs only a short "has it changed?" request, and an
interrupted download continues instead of starting over. The page cache holds at most 64 MB in
memory, and decoded images at most 192 MB.

## Known gaps and limitations

rynx is a beta. What's missing or rough today:

- **No JavaScript, by design.** Sites that only work with it (many web apps, most logins,
  CAPTCHAs) won't work; `A` (the Internet Archive's copy) or reader mode (`R`) sometimes help.
  Anubis bot checks are passed natively, except the `hashx` kind.
- **Cookies are all or nothing per site:** a site you allow (`C`) gets its cookies sent with every
  request to it, including images other pages load from it; there are no per-cookie choices and
  no third-party cookie rules beyond "only allowed sites". Saved cookies aren't encrypted (see
  encryption at rest on the roadmap).
- **Images:** SVG isn't supported; an image only partly on screen shows as its caption until it's
  scrolled fully into view; macOS Terminal can only show rough coloured blocks. Forcing an image
  protocol your terminal doesn't support can make it miss your first key presses.
- **Platforms:** Windows has no sandbox yet, and the macOS and Windows builds aren't code-signed,
  so the systems warn on first run. The sandbox has been checked on few Linux kernels and macOS
  versions so far.
- **Small web:** files can't be downloaded from Gemini or Gopher, and Gemini client certificates
  (used by some capsules for accounts) aren't supported. View source on Gemini, Gopher and feed
  pages shows the converted HTML, not the original.
- **Site tools (MCP):** no sign-in (OAuth), and servers can't ask questions while a tool runs.
- **The I2P router:** rynx starts i2pd but doesn't bundle it (install it yourself); on Windows,
  i2pd is found only through `i2pd_path` or `PATH`, and is untested there. Changes to the router
  settings take effect when rynx next starts. A router that stops when rynx quits leaves nothing
  behind on macOS only if rynx quits normally or panics; if rynx is killed, the next start stops it.
- **Smaller things:** the history page shows times in UTC. A proxy set in the system's settings
  (`HTTPS_PROXY` and the like) looks names up itself, so the local-network check for names
  doesn't apply through it (addresses are still checked).
- **Untested by hand:** [TEST_LOG.md](TEST_LOG.md) lists the checks still waiting for a person
  with the right system (I2P, other terminals, Linux distributions, Windows).

## Development

```sh
scripts/check.sh --quick     # formatting, lints and tests: before every commit
scripts/check.sh             # everything CI checks
scripts/release.sh 0.2.0-beta.7    # a release, start to finish
```

[CONTRIBUTING.md](CONTRIBUTING.md) covers building, testing, fuzzing (`fuzz/`), the conventions
and the release process; [SECURITY.md](SECURITY.md) how to report a vulnerability privately.
[TESTING.md](TESTING.md) has the hands-on checks, with results in [TEST_LOG.md](TEST_LOG.md), and
[CHANGELOG.md](CHANGELOG.md) lists the changes.

CI tests every push and pull request on Linux, macOS and Windows, with lints, renderer fuzzing,
`cargo deny` (`deny.toml`), the oldest supported Rust (1.90) and test coverage (about 87% of lines); coverage-guided fuzzing runs
nightly and the tests that need the internet weekly. Dependabot proposes updates weekly, and
actions are pinned to commit hashes.

A release (`scripts/release.sh <version>`, or a pushed `v*` tag) is checked and tested, built for
every platform, published on GitHub with checksums, build provenance attestations and a signature
of the checksums, and published to crates.io by trusted publishing. Tags containing `-` become
pre-releases. The checksums are signed with the release key (the `RELEASE_SIGNING_KEY` secret; its
public half is `RELEASE_KEY` in `src/update.rs` and `.github/release-signing-key.pub.pem`), and the
workflow checks the signature before publishing. If the key is ever replaced, older copies of rynx
can't verify newer releases and need one manual update.

## Roadmap

Done:

- [x] **I2P integration** through a local proxy
- [x] **Downloads** with progress, safe filenames and OS security tagging
- [x] **Clipboard** support for the address bar and page text
- [x] **Find on page** with highlighting
- [x] **Rendering** of tables, lists, quotes, code and plain-text files
- [x] **Forms** for searching and other text input
- [x] **Images**: Kitty, iTerm2, Sixel, or coloured text blocks; inline or full screen
- [x] **Bookmarks**
- [x] **Settings file** (home page, search engine, I2P proxy, downloads folder)
- [x] **Persistent history** (opt-in)
- [x] **Gopher, Gemini and RSS** support
- [x] **MCP tools** offered by sites, as an alternative to JavaScript
- [x] **Reader mode**, **Anubis** bot checks without JavaScript, a **settings page**
- [x] **Page cache**, **snapshots** of tracked sites, **resumable downloads**
- [x] **Sandbox** on Linux and macOS
- [x] **Signed in-app updates**, coverage-guided fuzzing, release and check scripts
- [x] **Customisable key bindings** (`keys.conf`)
- [x] **Starting the I2P router** (i2pd) with rynx

Next:

- [ ] **Your own MCP servers**: tools from servers you configure, usable on any page

Later:

- [ ] **A built-in I2P router** (the Rust router emissary, once it matures), so I2P needs
      nothing installed

- [ ] **MCP sign-in** (OAuth) for tools that need an account
- [ ] **Session restore** (opt-in): reopen your tabs
- [ ] **Encryption at rest** for history, the cache and snapshots, with a passphrase
- [ ] **SVG images**, and drawing images that are partly on screen
- [ ] **Downloads from Gemini and Gopher**, and Gemini client certificates
- [ ] **A Windows sandbox**, and **code-signed** macOS and Windows builds
- [ ] **Packages**: Homebrew, the AUR and others

If you want to contribute, open an issue or pull request on GitHub.

## License

rynx is free software, licensed under the GNU General Public License, version 3 or (at your
option) any later version (GPL-3.0-or-later). See [LICENSE.txt](LICENSE.txt).

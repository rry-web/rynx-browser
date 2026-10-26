# Manual Testing Checklist

Hands-on checks for the bug fixes and security hardening done in October 2026. Run `cargo test`
first; most of these behaviours also have automated tests. This list covers what tests can't
fully show: real terminals, touchpads, live sites, macOS Finder and I2P.

Record results in [TEST_LOG.md](TEST_LOG.md) using the IDs below.

Tip: `cargo run --release` renders large pages about 6× faster than a debug build.

Items marked **(priority)** could not be checked at all during development, or only partly.

## Automated checks

These run in CI (`.github/workflows/ci.yml`) on every push and weekly, but can be run locally.

- [ ] **AUTO-01 Test suite.** `cargo test`: all tests pass, including 100 randomised renderer cases.
- [ ] **AUTO-02 Dependency audit.** `cargo audit`: no known vulnerabilities in dependencies.
- [ ] **AUTO-03 Live TLS checks.** `cargo test --test live_tls -- --ignored` (needs internet): expired,
      self-signed, wrong-host, untrusted and (on macOS/Windows) revoked certificates are
      rejected; TLS 1.0/1.1 are refused; HTTP/2 still works.
- [ ] **AUTO-06 I2P TLS settings.** `cargo test --lib -- --ignored` (needs internet): the TLS
      configuration used for I2P connections (bundled root certificates) connects over HTTP/2
      and rejects bad certificates and TLS 1.1, checked without needing an I2P router.
- [ ] **AUTO-07 Live Gemini, Gopher and feeds.** `cargo test --test smallweb --test feeds -- --ignored`
      (needs internet): geminiprotocol.net, gopher.floodgap.com and the Rust blog's feed load and
      convert.
- [ ] **AUTO-08 Live MCP server.** `cargo test --test mcp -- --ignored` (needs internet): lists
      DeepWiki's public MCP tools and runs one.
- [ ] **AUTO-04 Extended fuzzing.** `FUZZ_ITERATIONS=100000 cargo test --release --test fuzz_render`: randomised
      renderer and UI cases (about 4 minutes). Reproduce a failure with `FUZZ_SEED=<seed>`.
- [ ] **AUTO-05 (priority) CI runs on GitHub.** After pushing to GitHub, open the repository's **Actions** tab.
      *Expected:* the CI workflow runs and all jobs (Linux and macOS tests, fuzzing, audit)
      pass. It has never run, and the Linux build has not been tried.

## Local test servers

Some checks need deliberately hostile pages. These serve them from your own machine.
`localhost` is allowed over plain HTTP, so the browser can reach them.

```sh
mkdir -p /tmp/rynx-test && cd /tmp/rynx-test

# A page with terminal escape codes and non-web links
printf '<title>Evil\033]0;TITLE CHANGED\007</title><p>Before\033]52;c;cHduZWQ=\007 after\033[2J</p><p><a href="mailto:someone@example.com">mail link</a> <a href="javascript:alert(1)">js link</a> <a href="http://secret-site.i2p/">i2p link</a></p>' > evil.html

# 60 nested lists: used to freeze the browser at any terminal width
python3 -c 'print("<ul>" * 60 + "<li>deep item" + "</ul>" * 60)' > nested.html

# 20,000 nested elements: used to crash the browser with a stack overflow
python3 -c 'print("<span>" * 20000 + "deep text" + "</span>" * 20000)' > deep.html

# 50,000 nested <div>s: takes seconds to parse, and used to freeze the UI meanwhile
python3 -c 'print("<div>" * 50000 + "slow page" + "</div>" * 50000)' > slow.html

# A plain-text file with a URL, and a "binary" file
printf '<b>not markup</b>\n\n    indented line\nSee https://example.com/docs.\n' > notes.txt
cp /System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/GenericDocumentIcon.icns icon.icns 2>/dev/null || head -c 2048 /dev/urandom > icon.icns

# A "compression bomb": ~20KB on the wire that expands to ~20MB, sent without Content-Length
cat > bomb_server.py <<'EOF'
import gzip, http.server
body = gzip.compress(b"<p>" + b"a " * (10 * 1024 * 1024) + b"</p>")
class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "text/html")
        self.send_header("Content-Encoding", "gzip")
        self.end_headers()
        self.wfile.write(body)
print("serving on http://localhost:8001/", flush=True)
http.server.HTTPServer(("127.0.0.1", 8001), Handler).serve_forever()
EOF

python3 -m http.server 8000 --bind 127.0.0.1 &   # serves the files above
python3 bomb_server.py &                          # serves the bomb
```

Stop them afterwards with `kill %1 %2` (or `pkill -f http.server; pkill -f bomb_server.py`).

## General smoke test

Worth repeating after any dependency upgrade.

- [ ] **SMOKE-01 Key bindings.** Every key binding in the README does what it says: tabs (`n`, `t`, `w`,
      `[`, `]`), Edit mode (`e`), search (`/`, `>`, `<`), visual mode (`v`, `y`), source view
      (`Shift+V`), I2P toggle (`p`), quit (`q`).
- [ ] **SMOKE-02 Mouse.** scroll wheel scrolls; clicking a link follows it; `Ctrl`+click
      opens it in a new tab.
- [ ] **SMOKE-03 Resizing.** Resizing the terminal re-wraps the page without glitches.
- [ ] **SMOKE-04 Terminal restored on exit.** After quitting, the terminal is back to normal (cursor visible, typing
      echoes, no leftover colours).

## Rendering

- [ ] **RENDER-01 Documentation pages.** Open a page with inline code and code blocks, e.g.
      `https://doc.rust-lang.org/std/vec/struct.Vec.html`.
      *Expected:* sentences with `inline code` read as normal sentences (no line break at each
      code word); code blocks keep their lines and indentation, one line of code per line.
- [ ] **RENDER-02 Data tables.** Open a Wikipedia article with an infobox or table, e.g.
      `https://en.wikipedia.org/wiki/Rust_(programming_language)`.
      *Expected:* table cells line up in columns separated by `│`, header rows are bold with a
      `─┼─` rule underneath, and long cells wrap inside their column.
- [ ] **RENDER-03 Narrow tables.** Shrink the terminal to about 30 columns on the same page.
      *Expected:* tables that no longer fit switch to `Header: value` lines per row.
- [ ] **RENDER-04 Layout tables.** Open `https://news.ycombinator.com/`.
      *Expected:* ranked titles with their points/comments line underneath; the header reads
      "Hacker News new | past | ..." (words not glued together).
- [ ] **RENDER-05 Lists and quotes.** Find a page with numbered lists, nested lists and
      blockquotes (Wikipedia talk pages, mailing-list archives or GitHub READMEs).
      *Expected:* numbered lists show `1.`, `2.` (right-aligned past 9), nested bullets change
      (•, ◦, ▪), wrapped items line up under their text, and quotes have a `│` bar.
- [ ] **RENDER-06 Plain-text files.** Open `https://www.rfc-editor.org/rfc/rfc1149.txt`, and
      `http://localhost:8000/notes.txt` with the test servers running.
      *Expected:* the RFC keeps its indentation and layout; in notes.txt, `<b>not markup</b>`
      shows literally and the URL is a selectable link (without the trailing period).
- [ ] **RENDER-07 JSON.** Open `https://httpbin.org/json`.
      *Expected:* the JSON keeps its line breaks and indentation.
- [ ] **RENDER-08 Non-text files.** Open `http://localhost:8000/icon.icns` (or any image URL).
      *Expected:* a page naming the file and its type with a download link, instead of garbage;
      selecting the link and pressing `d` offers to save it.
- [ ] **RENDER-11 Site menus.** Open `https://www.bbc.com/news` and `https://en.wikipedia.org/`.
      *Expected:* the navigation menus at the top are compact lines like
      `Home · News · Sport · …` (submenus as `News: US & Canada · UK · …`) rather than long
      columns of single links, and each entry is still selectable with Tab. Lists in the article
      itself (e.g. "See also", infobox entries) are still drawn as bulleted lists.
- [ ] **RENDER-09 Full width.** On any text-heavy page, check the right edge.
      *Expected:* text runs to within a column of the right border (it used to stop 2 short).
- [ ] **RENDER-10 No-JavaScript notices.** Visit a site that relies on JavaScript.
      *Expected:* its `<noscript>` message shows as normal text, not as raw `<p>` tags.

## Crashes and stability

- [ ] **CRASH-01 Stale link selection.** On a long page, press `Tab` many times to select a
      link far down. Press `e`, type a URL for a short page, and press Enter.
      *Expected:* the new page loads; no crash.
- [ ] **CRASH-02 Tiny terminal.** Start a download (`d` on a file link, then `y`), then shrink
      the terminal window as small as it goes.
      *Expected:* no crash; the popup is clipped.
- [ ] **CRASH-03 Visual mode on an empty page.** In a new tab (`n`), press `Shift+V` for
      source view, then `v` and `j`/`k`/`l`.
      *Expected:* no crash.
- [ ] **CRASH-04 Deeply nested lists.** With the test servers running, open
      `http://localhost:8000/nested.html`.
      *Expected:* "deep item" appears promptly; the browser stays responsive.
- [ ] **CRASH-05 Extremely narrow terminal.** On any page, drag the terminal window as narrow
      as it goes (2 columns or less), then widen it again.
      *Expected:* the browser stays responsive and recovers. It used to freeze.
- [ ] **CRASH-06 Extremely deep nesting.** With the test servers running, open
      `http://localhost:8000/deep.html`.
      *Expected:* "deep text" appears; no crash.

## Navigation and history

- [ ] **NAV-01 Back after typing a URL.** Load a page, press `e`, type a different URL,
      Enter, then `Backspace`.
      *Expected:* you return to the first page, not a reload of the second.
- [ ] **NAV-02 Esc cancels editing.** Press `e`, type some junk, press `Esc`.
      *Expected:* the address bar shows the original address again.
- [ ] **NAV-03 Redirects update the address bar.** Open
      `https://www.gutenberg.org/ebooks/2701.html.images`.
      *Expected:* the address bar ends up showing
      `https://www.gutenberg.org/cache/epub/2701/pg2701-images.html` (note `https`; the
      site's redirect actually points to `http://`).
- [ ] **NAV-04 Chapter links jump within the page.** On that Moby Dick page, select a chapter
      link in the table of contents and press Enter (or click it).
      *Expected:* the view jumps straight to that chapter's heading with no reload. The
      address bar ends in `#link2HCH...`, and `Tab` selects the next link after the heading.
- [ ] **NAV-05 Back from a chapter.** After jumping to a chapter, press `Backspace`.
      *Expected:* back to the top of the page instantly, with no reload.
      (Known limitation: it returns to the top, not to where you were scrolled.)
- [ ] **NAV-06 Fragment on another page.** Open a URL with a `#section` that points into a
      different page, by typing one or following a link from another site.
      *Expected:* once it loads, the view starts at that section.
- [ ] **NAV-07 Fragment link in a new tab.** On the Moby Dick page, select a chapter link
      and press `t` (or `Ctrl`+click it).
      *Expected:* a new tab opens, loads the book and starts at that chapter.
- [ ] **NAV-08 Redirects within a local server.** With the test servers running, run
      `mkdir /tmp/rynx-test/dir` and open `http://localhost:8000/dir` (the server redirects it
      to `/dir/`).
      *Expected:* the directory listing loads and the address bar ends in `/dir/`.
      Local-to-local redirects used to be blocked.
- [ ] **NAV-09 HTTP-only sites are not downgraded.** Open `http://` followed by a site that
      only serves plain HTTP, such as `http://neverssl.com`.
      *Expected:* the browser tries `https://` and shows an error, rather than loading the
      page over plain HTTP.

## Forms

- [ ] **FORM-01 Search box.** Open `https://en.wikipedia.org/`, press `Tab` until the search field
      is selected, press Enter, type a word, press Enter.
      *Expected:* the bottom bar showed `Text field "search" → sends to …/w/index.php` before you
      typed; the search results page loads.
- [ ] **FORM-02 Search in a table.** On `https://news.ycombinator.com/`, scroll to the bottom and
      search with the "Search:" field.
      *Expected:* hn.algolia.com opens with your query.
- [ ] **FORM-03 Checkboxes, options and choices.** On a page with a form that has them (e.g. a
      search engine's advanced search), use Enter on each.
      *Expected:* checkboxes toggle `[x]`, picking an option clears the others in its group,
      choices cycle through their options.
- [ ] **FORM-04 Passwords.** On any login form, type into the password field.
      *Expected:* only `•` characters are shown. (Logging in won't stick: there are no cookies.)
- [ ] **FORM-06 Forms can't reach your local network.** Open your router's admin page (e.g.
      `http://192.168.1.1/`) and use a form there.
      *Expected:* it works: local pages may submit to local addresses. (The other direction, a
      public site's form aimed at your router or `localhost`, is refused with "can't send data to
      your local network"; that needs a hostile public page, so automated tests cover it.)
- [ ] **FORM-07 Disguised links.** Open a page containing a link like
      `https://paypal.com@example.com/` (e.g. add one to `evil.html` on the test server).
      *Expected:* the bottom bar warns "⚠ really goes to example.com"; Enter refuses to follow it
      and names the real site.
- [ ] **FORM-08 Passwords in typed addresses.** Type `https://user:secret@example.com/` into the
      address bar and press Enter, then press `b`.
      *Expected:* the address bar shows `user:••••@`, and the bookmark is saved without the
      user name or password.
- [ ] **FORM-05 Typing keeps up.** Type quickly into a field on a large page (e.g. Wikipedia).
      *Expected:* characters appear immediately.

## History and paging

- [ ] **HIST-01 Back keeps your place.** Scroll partway down a long page, follow a link, then go
      back.
      *Expected:* you return to the same place on the first page, not its top.
- [ ] **HIST-02 Forward and reload.** After going back, press `Right` (or `L`); then press `r`.
      *Expected:* forward returns to the second page where you left it; reload keeps your place.
- [ ] **HIST-03 Paging keys.** On a long page, try `PageDown`/`PageUp`, `Ctrl+d`/`Ctrl+u`, `G`,
      `g`.
      *Expected:* screen / half-screen moves, bottom and top; `Ctrl+d` never starts a download.
- [ ] **HIST-04 Links to /files/ pages.** Click an ordinary link whose address contains `/files/`
      or `/download/` but ends in `.html` or `/`.
      *Expected:* the page opens (no download prompt). Links ending in `.zip`, `.pdf` etc. still go
      to the download prompt.

## Settings and bookmarks

- [ ] **SET-01 Settings template.** After the first run, open `settings.conf` in your config
      folder (see the README).
      *Expected:* it exists with every option commented out.
- [ ] **SET-02 Search engine.** Set `search_url = https://html.duckduckgo.com/html/?q={query}`,
      restart, press `e`, type some words and press Enter.
      *Expected:* DuckDuckGo results appear directly.
- [ ] **SET-03 Home page.** Set `home_page`, restart, and press `n`.
      *Expected:* the browser and new tabs open the home page.
- [ ] **SET-04 Bad settings.** Add a line `colour = blue` and restart.
      *Expected:* the status line reports the problem; everything else works.
- [ ] **SET-05 Bookmarks.** Press `b` on a page, then `B`.
      *Expected:* the bookmarks page lists it; selecting it opens the page. Restart and press `B`:
      it's still there. Pressing `b` on that page again removes it.
- [ ] **SET-06 (priority) Bookmarks under the Linux sandbox.** On Linux with the sandbox active,
      add a bookmark and restart.
      *Expected:* the bookmark was saved (the settings folder is writable in the sandbox).
- [ ] **SET-11 User-Agent.** Open `https://git.kernel.org/`.
      *Expected:* the repository list, not an Anubis "Making sure you're not a bot" page. Then set
      `user_agent` to a browser's string (see the template in settings.conf), restart, and open
      it again: now the challenge page appears. Remove the setting afterwards.
- [ ] **SET-12 (priority) Anubis.** Open `https://www.royalroad.com/` and a story page there.
      *Expected:* "Passing www.royalroad.com's bot check (Anubis), without JavaScript..." for a
      moment, then the real page (not "Making sure you're not a bot"). Later pages on the site
      load straight away. With `anubis = off`, the check's page shows and the status line says
      passing is turned off. This has only been tested against a simulated Anubis.
- [ ] **SET-13 Copy mode.** On any page press `v`, then the arrow keys, then `y`.
      *Expected:* the address bar's title reads COPY MODE (black on cyan) with the number of
      characters selected, the bar explains the keys, and the selection is black on cyan and easy
      to read. `y` reports "Copied N characters to the clipboard"; the address comes back.
- [ ] **SET-07 Session history.** With no `history` setting, visit a few pages and press `s`.
      *Expected:* the history page lists them newest first, under a date heading, with their
      titles; selecting one opens it. Restart and press `s`: it's empty, and no `history.txt`
      exists in the config folder.
- [ ] **SET-08 Saved history.** Set `history = on`, restart, visit a few pages, quit and restart.
      *Expected:* `s` still lists them. Visit one in I2P mode too: it's listed until you quit,
      but never appears in `history.txt`.
- [ ] **SET-09 Clearing history.** On the history page press `x`, then another key, then `x`
      twice.
      *Expected:* the first `x` asks to press again; the other key cancels; two `x` presses in a
      row empty the page and `history.txt`.
- [ ] **SET-10 (priority) History under the Linux sandbox.** On Linux with the sandbox active and
      `history = on`, visit a page and restart.
      *Expected:* the page is still in the history.

## Scrolling and input

- [ ] **SCROLL-01 (priority) Touchpad scrolling keeps up.** On the Moby Dick page, scroll
      hard with the touchpad, then stop.
      *Expected:* scrolling stops when your fingers stop. (A short glide can be macOS
      momentum scrolling, which the terminal sends itself.)
- [ ] **SCROLL-02 Scrolling stops at the end.** Scroll to the very bottom of the book with
      the wheel, then with the `Down` arrow.
      *Expected:* both stop with the last line at the bottom of the view; scrolling back up
      responds immediately.
- [ ] **SCROLL-03 `j`/`k` continue from the view.** Scroll down a long way with the wheel,
      then press `j` and `k`.
      *Expected:* the cursor moves from what's on screen; the view doesn't jump to the top.
- [ ] **SCROLL-04 Visual mode follows the cursor.** Press `v`, then hold `j` past the bottom
      of the screen.
      *Expected:* the view scrolls and the selection keeps extending.
- [ ] **SCROLL-05 Wheel scrolling in visual mode.** Press `v`, then scroll down with the
      wheel.
      *Expected:* the selection extends with the view.
- [ ] **SCROLL-06 Clicking below the page.** On a short page, click in the empty area below
      the last line, then press `k`.
      *Expected:* the cursor is on the last line and moves up from there.
- [ ] **SCROLL-07 (priority) Quitting while moving the mouse.** Move your finger on the
      touchpad and press `q` at the same time.
      *Expected:* the browser exits immediately, with no `35;…M` garbage in your shell.
- [ ] **SCROLL-08 Search survives a typo.** Press `/`, type something not on the page.
      *Expected:* the search bar shows `[0/0]`. `Backspace` and retyping still works.
- [ ] **SCROLL-09 Search jumps.** Search for a common word, press Enter, then `>` and `<`
      several times.
      *Expected:* the current match (green) moves and the view scrolls to keep it visible.
- [ ] **SCROLL-10 Source view cursor.** Press `Shift+V`, then move with `h`/`j`/`k`/`l`.
      *Expected:* the cursor highlights a single character, not the whole line.

## Performance and memory

- [ ] **PERF-01 Large pages.** With `cargo run --release`, open the Moby Dick page and scroll
      through it.
      *Expected:* it loads in well under a second and scrolls smoothly.
- [ ] **PERF-02 Memory use.** With the Moby Dick page open, check the browser's memory in
      Activity Monitor (or `top`).
      *Expected:* tens of MB, not hundreds. In testing, rendering it used about 19MB (93MB
      before the fix).
- [ ] **PERF-03 Slow pages don't freeze the UI.** With the test servers running, open
      `http://localhost:8000/slow.html` and, while the status reads "Rendering...", press
      `Tab`, scroll, switch tabs, or press `q`.
      *Expected:* everything responds immediately; "slow page" appears after a few seconds
      (unless you quit, which exits at once).
- [ ] **PERF-04 Help page after resizing.** In a new tab (`n`), resize the window.
      *Expected:* the help text re-wraps to the new width instead of disappearing.

## Downloads

- [ ] **DL-01 Speed and filename.** Download a large file (tens of MB).
      *Expected:* it downloads at normal network speed, and the popup shows the real
      filename, not "Downloading...".
- [ ] **DL-02 Query strings.** Download a link like `.../file.zip?token=abc`.
      *Expected:* saved as `file.zip`.
- [ ] **DL-03 Error pages aren't saved.** Download a link that returns 404.
      *Expected:* "Download Failed" with the status code; no file in `~/Downloads`; the page
      you were on stays visible.
- [ ] **DL-04 No silent overwrite.** Download a file whose URL has no extension (e.g. an
      image at `/photo`) while a file with the detected name (e.g. `photo.png`) already
      exists in `~/Downloads`.
      *Expected:* the new file is saved as `photo_1.png`; the existing file is untouched.
- [ ] **DL-05 One download per tab.** Start a large download, then press `d` on another link
      in the same tab.
      *Expected:* "A download is already in progress in this tab".
- [ ] **DL-06 (priority) Gatekeeper checks downloads (macOS).** Download a small unsigned app
      or script (e.g. a `.zip` containing a `.app` from a GitHub release), then open it from
      Finder.
      *Expected:* macOS shows its "downloaded from the internet" warning.
      `xattr -l ~/Downloads/<file>` shows `com.apple.quarantine: 0081;...;rynx;`.
- [ ] **DL-07 HTTP download links are upgraded.** Download a file from an `http://` link to a
      public site.
      *Expected:* it's fetched over `https://`. If the site has no HTTPS, it fails instead of
      downloading over plain HTTP.

## Clipboard

- [ ] **CLIP-01 Copy and paste still work.** In Edit mode: `Ctrl+y` copies the address,
      `Ctrl+v` pastes, `Ctrl+k` replaces the address with the clipboard. In visual mode, `y`
      copies the selection.
- [ ] **CLIP-02 (priority) No clipboard available (Linux).** Run with no display:
      `DISPLAY= WAYLAND_DISPLAY= cargo run`, or over SSH.
      *Expected:* the browser starts. Copying shows "Clipboard unavailable", and `Ctrl+k`
      leaves the address alone.

## Security

- [ ] **SEC-01 (priority) Escape codes are neutralised.** With the test servers running, open
      `http://localhost:8000/evil.html`. Best done in iTerm2, kitty or WezTerm, which honour
      clipboard escape codes.
      *Expected:* the window title doesn't change to "TITLE CHANGED", the screen isn't
      cleared, and your clipboard is **not** replaced with "pwned". The page shows
      `Before]52;c;cHduZWQ= after[2J`, i.e. the escape characters removed and the rest as
      plain text.
- [ ] **SEC-02 Link destinations are shown.** On any page, press `Tab` to select links.
      *Expected:* the bottom border of the page shows `Link: <full URL>` for the selected
      link, with relative links resolved. Look-alike domains appear as `xn--...` punycode.
- [ ] **SEC-03 Non-web links are refused.** On `evil.html`, follow "mail link" and "js link"
      with Enter, `t` and click.
      *Expected:* "Unsupported link type: mailto" / "javascript"; no request, no new tab.
- [ ] **SEC-04 `.i2p` blocked outside I2P mode.** With I2P mode off, follow "i2p link" on
      `evil.html`.
      *Expected:* an error page saying "Enable I2P mode (press p) to visit .i2p sites".
- [ ] **SEC-05 Compression bomb rejected.** Open `http://localhost:8001/`.
      *Expected:* "Page too large (over 10 MB)" without a big memory spike.
- [ ] **SEC-06 Non-UTF-8 pages still render.** Visit an older site that declares
      `charset=windows-1252` or `ISO-8859-1`.
      *Expected:* accented characters display correctly.
- [ ] **SEC-07 Revoked certificates are rejected.** Open `https://revoked.badssl.com/`.
      *Expected:* an error mentioning the certificate is revoked (macOS and Windows).

## Platform security

- [ ] **PLAT-01 (priority) Linux sandbox status.** On Linux, start the browser.
      *Expected:* the status line reads "Ready (filesystem sandbox active)". On kernels older than
      5.13 it says the kernel lacks Landlock instead.
- [ ] **PLAT-02 (priority) Linux sandbox doesn't break browsing.** With the sandbox active, browse
      a few HTTPS sites, download a file, and copy text with `v`/`y`.
      *Expected:* all of it works; the download lands in `~/Downloads`. If anything fails, note
      the distribution, then retry with `--no-sandbox` to confirm the sandbox is the cause.
- [ ] **PLAT-03 Linux download origin.** After a download, run
      `getfattr -n user.xdg.origin.url ~/Downloads/<file>`.
      *Expected:* it prints the URL the file came from.
- [ ] **PLAT-04 (priority) Windows Mark of the Web.** On Windows, download a file, then in
      PowerShell run `Get-Content <file> -Stream Zone.Identifier`.
      *Expected:* it shows `ZoneId=3` and the download URL; opening a downloaded program
      triggers a SmartScreen check.
- [ ] **PLAT-05 macOS "Where from".** On macOS, download a file and open Finder's Get Info on it.
      *Expected:* "Where from" shows the download URL.
- [ ] **PLAT-06 Device-name downloads.** Download a link whose file is named `con.txt` or `NUL`.
      *Expected:* it's saved as `_con.txt` / `_NUL`.
- [ ] **PLAT-07 Command-line options.** Run `rynx --version` and `rynx --help`.
      *Expected:* the version (`0.2.0-beta.1`) and a usage summary; an unknown option prints an
      error and the usage.

## Releases

- [ ] **REL-01 (priority) First beta release.** Push the version tag:
      `git tag v0.2.0-beta.1 && git push origin main v0.2.0-beta.1`, then watch the **Release**
      workflow in the Actions tab.
      *Expected:* all six builds pass (tests run on five), and a **pre-release** appears on the
      releases page with six archives, `SHA256SUMS`, and notes from CHANGELOG.md.
- [ ] **REL-02 (priority) Verify a downloaded build.** Download your platform's archive and
      `SHA256SUMS`, then run `sha256sum -c SHA256SUMS --ignore-missing` and
      `gh attestation verify <archive> --repo rry-web/rynx-browser`.
      *Expected:* the checksum matches and the attestation verifies.
- [ ] **REL-03 (priority) Run each platform's build.** On each OS you have, extract and run the
      binary, following the release notes for the unsigned-binary warning.
      *Expected:* it starts and browses normally.

## Gemini, Gopher and feeds

- [ ] **SMALL-01 Gemini.** Press `e`, type `gemini://geminiprotocol.net/` and press Enter.
      *Expected:* the page shows with `⇒` before each link, one per line; `Tab` and Enter follow
      them, and Back returns.
- [ ] **SMALL-02 (priority) Gemini input.** Open a Gemini search engine such as
      `gemini://geminispace.info/search`.
      *Expected:* a page with the prompt and a text field. Press Enter, type some words, press
      Enter: the results appear, and the address ends in `?your%20words`.
- [ ] **SMALL-03 Gemini certificates.** After SMALL-01, open `gemini_known_hosts.txt` in the
      config folder. Change the fingerprint after `geminiprotocol.net:1965` to zeros, save,
      and reload the page (`r`).
      *Expected:* the host was listed. With the changed fingerprint the page is refused with
      "certificate has changed" (unless the site's certificate is CA-signed, in which case it
      loads and the fingerprint is corrected). Delete the line to visit it normally again.
- [ ] **SMALL-04 Gopher.** Open `gopher://gopher.floodgap.com/`.
      *Expected:* the menu's text art lines up, items are marked `[DIR]`, `[TXT]`, `[ ? ]`;
      a `[TXT]` item opens as text; a `[ ? ]` search item asks for words and shows results.
- [ ] **SMALL-05 Not in I2P mode.** Press `p`, then open a `gemini://` or `gopher://` address.
      *Expected:* "Gemini and Gopher aren't available in I2P mode", and nothing is fetched.
- [ ] **SMALL-06 Feeds.** Open `https://blog.rust-lang.org/feed.xml`.
      *Expected:* the blog's title, "Feed with N items", then each post with its date and text.
      Open `https://blog.rust-lang.org/`: the first line reads `Feeds:` with a link to the feed.
- [ ] **SMALL-07 Bookmarks and history.** Bookmark a Gemini page (`b`), then check `B` and `s`.
      *Expected:* both list the page by its first heading, and selecting it opens it again.

## Site tools (MCP)

- [ ] **MCP-01 Listing tools.** Press `e` and type `https://mcp.deepwiki.com/mcp`.
      *Expected:* a page saying it's an MCP server, with an "Open its tools" link (not a "Not
      Acceptable" error). Following it shows "Tools from mcp.deepwiki.com" with three tools, each with a description, its
      fields marked "(required)" where needed, and a Run button.
- [ ] **MCP-02 (priority) Confirming a call.** In "read_wiki_structure", type `rust-lang/rust`
      into repoName and press Enter.
      *Expected:* a "Run a tool" box shows `"repoName": "rust-lang/rust"` and a warning that the
      tool may change things (DeepWiki doesn't mark it read-only). Clicking and other keys do
      nothing; `n` cancels with "nothing was sent". Enter again, then `y`: the list of wiki pages
      appears, with nested items indented.
- [ ] **MCP-03 Back and forward.** From the result press Backspace, then Right.
      *Expected:* Back shows the tools again; Forward shows the same result without running the
      tool again.
- [ ] **MCP-04 Validation.** Run a tool with a required field empty.
      *Expected:* the status line says to fill it in, and no box appears.
- [ ] **MCP-05 Download prompt.** Press `d` on a link, then press `q` and `j`.
      *Expected:* the download box stays open and nothing else happens; `n` closes it.

## I2P (needs a local I2P router on 127.0.0.1:4444)

- [ ] **I2P-01 (priority) Browsing in I2P mode.** Press `p` and load an `.i2p` site.
      *Expected:* it loads through the proxy, and the URL bar shows `[I2P MODE ON]`.
- [ ] **I2P-02 (priority) HTTPS through I2P.** In I2P mode, load an `https://` site (via an
      outproxy, or an I2P site that offers HTTPS).
      *Expected:* it loads. This connection uses rustls with TLS 1.2 minimum and the bundled
      root certificates, and has not been tested against a real router.
- [ ] **I2P-03 (priority) Jump services.** In I2P mode, open an `.i2p` address your router
      doesn't know yet.
      *Expected:* "Requesting Jump Helper..." and the jump services are tried in turn.
      Clearweb sites with server errors never trigger a jump lookup.
- [ ] **I2P-04 (priority) Toggling back.** Press `p` again and load a clearweb site.
      *Expected:* it loads directly (URL bar shows `[Clearweb]`), over HTTPS.

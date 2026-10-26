# Manual Testing Checklist

Hands-on checks for the fixes on the `fix/core-bugs` branch. Run `cargo test` first; most of
these behaviours also have automated tests. This list covers what tests can't fully show:
real terminals, touchpads, live sites and I2P.

Tip: `cargo run --release` renders large pages about 6× faster than a debug build.

Items marked **(priority)** could not be checked at all during development, or only partly.

## Local test servers

Some security checks need a deliberately hostile page. These serve one from your own machine.
`localhost` is allowed over plain HTTP, so the browser can reach them.

```sh
mkdir -p /tmp/rynx-test && cd /tmp/rynx-test

# A page with terminal escape codes and non-web links
printf '<title>Evil\033]0;TITLE CHANGED\007</title><p>Before\033]52;c;cHduZWQ=\007 after\033[2J</p><p><a href="mailto:someone@example.com">mail link</a> <a href="javascript:alert(1)">js link</a> <a href="http://secret-site.i2p/">i2p link</a></p>' > evil.html

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

python3 -m http.server 8000 --bind 127.0.0.1 &   # serves evil.html
python3 bomb_server.py &                          # serves the bomb
```

Stop them afterwards with `kill %1 %2` (or `pkill -f http.server; pkill -f bomb_server.py`).

## Crashes and stability

- [ ] **Stale link selection.** On a long page, press `Tab` many times to select a link far
      down. Press `e`, type a URL for a short page, and press Enter.
      *Expected:* the new page loads; no crash.
- [ ] **Tiny terminal.** Start a download (`d` on a file link, then `y`), then shrink the
      terminal window as small as it goes.
      *Expected:* no crash; the popup is clipped.
- [ ] **Visual mode on an empty page.** In a new tab (`n`), press `Shift+V` for source view,
      then `v` and `j`/`k`/`l`.
      *Expected:* no crash.

## Navigation and history

- [ ] **Back after typing a URL.** Load a page, press `e`, type a different URL, Enter, then
      `Backspace`.
      *Expected:* you return to the first page, not a reload of the second.
- [ ] **Esc cancels editing.** Press `e`, type some junk, press `Esc`.
      *Expected:* the address bar shows the original address again.
- [ ] **Redirects update the address bar.** Open
      `https://www.gutenberg.org/ebooks/2701.html.images`.
      *Expected:* the address bar ends up showing
      `https://www.gutenberg.org/cache/epub/2701/pg2701-images.html` (note `https`).
- [ ] **Chapter links jump within the page.** On that Moby Dick page, select a chapter link in
      the table of contents and press Enter (or click it).
      *Expected:* the view jumps straight to that chapter's heading with no reload. The address
      bar ends in `#link2HCH...`, and `Tab` selects the next link after the chapter heading.
- [ ] **Back from a chapter.** After jumping to a chapter, press `Backspace`.
      *Expected:* back to the top of the page instantly, with no reload.
      (Known limitation: it returns to the top, not to where you were scrolled.)
- [ ] **Fragment on another page.** Open a URL with a `#section` that points into a different
      page, for example from another site's link, or by typing one.
      *Expected:* once it loads, the view starts at that section.

## Scrolling and input

- [ ] **(priority) Touchpad scrolling keeps up.** On the Moby Dick page, scroll hard with the
      touchpad, then stop.
      *Expected:* scrolling stops when your fingers stop. (A short glide can be macOS
      momentum scrolling, which the terminal sends itself.)
- [ ] **Scrolling stops at the end.** Scroll to the very bottom of the book.
      *Expected:* it stops with the last line at the bottom of the view.
- [ ] **`j`/`k` continue from the view.** Scroll down a long way with the wheel, then press
      `j` and `k`.
      *Expected:* the cursor moves from what's on screen; the view doesn't jump to the top.
- [ ] **Visual mode follows the cursor.** Press `v`, then hold `j` past the bottom of the
      screen.
      *Expected:* the view scrolls and the selection keeps extending.
- [ ] **(priority) Quitting while moving the mouse.** Move your finger on the touchpad and
      press `q` at the same time.
      *Expected:* the browser exits immediately, with no `35;…M` garbage in your shell.
- [ ] **Search survives a typo.** Press `/`, type something not on the page.
      *Expected:* the search bar shows `[0/0]`. `Backspace` and retyping still works.

## Downloads

- [ ] **Speed and filename.** Download a large file (tens of MB).
      *Expected:* it downloads at normal network speed, and the popup shows the real filename,
      not "Downloading...".
- [ ] **Query strings.** Download a link like `.../file.zip?token=abc`.
      *Expected:* saved as `file.zip`.
- [ ] **Error pages aren't saved.** Download a link that returns 404.
      *Expected:* "Download Failed" with the status code; no file in `~/Downloads`; the page
      you were on stays visible.
- [ ] **No silent overwrite.** Download a file whose URL has no extension (e.g. an image at
      `/photo`) while a file with the detected name (e.g. `photo.png`) already exists in
      `~/Downloads`.
      *Expected:* the new file is saved as `photo_1.png`; the existing file is untouched.
- [ ] **One download per tab.** Start a large download, then press `d` on another link in the
      same tab.
      *Expected:* "A download is already in progress in this tab".
- [ ] **(priority) Gatekeeper checks downloads (macOS).** Download a small unsigned app or
      script (e.g. a `.zip` containing a `.app` from a GitHub release), then open it from
      Finder.
      *Expected:* macOS shows its "downloaded from the internet" warning.
      `xattr -l ~/Downloads/<file>` shows `com.apple.quarantine: 0081;...;rynx_browser;`.
- [ ] **HTTP download links are upgraded.** Download a file from an `http://` link to a
      public site.
      *Expected:* it's fetched over `https://`. If the site has no HTTPS, it fails instead of
      downloading over plain HTTP.

## Clipboard

- [ ] **Copy and paste still work.** In Edit mode: `Ctrl+y` copies the address, `Ctrl+v`
      pastes, `Ctrl+k` replaces the address with the clipboard. In visual mode, `y` copies the
      selection.
- [ ] **(priority) No clipboard available (Linux).** Run with no display:
      `DISPLAY= WAYLAND_DISPLAY= cargo run`, or over SSH.
      *Expected:* the browser starts. Copying shows "Clipboard unavailable", and `Ctrl+k`
      leaves the address alone.

## Security

- [ ] **(priority) Escape codes are neutralised.** With the test servers running, open
      `http://localhost:8000/evil.html`. Best done in iTerm2, kitty or WezTerm, which honour
      clipboard escape codes.
      *Expected:* the window title doesn't change to "TITLE CHANGED", the screen isn't
      cleared, and your clipboard is **not** replaced with "pwned". The page shows
      `Before]52;c;cHduZWQ= after[2J`, i.e. the escape characters removed and the rest as
      plain text.
- [ ] **Link destinations are shown.** On any page, press `Tab` to select links.
      *Expected:* the bottom border of the page shows `Link: <full URL>` for the selected
      link, with relative links resolved. Look-alike domains appear as `xn--...` punycode.
- [ ] **Non-web links are refused.** On `evil.html`, follow "mail link" and "js link" with
      Enter, `t` and click.
      *Expected:* "Unsupported link type: mailto" / "javascript"; no request, no new tab.
- [ ] **`.i2p` blocked outside I2P mode.** With I2P mode off, follow "i2p link" on
      `evil.html`.
      *Expected:* an error page saying "Enable I2P mode (press p) to visit .i2p sites".
- [ ] **Compression bomb rejected.** Open `http://localhost:8001/`.
      *Expected:* "Page too large (over 10 MB)" without a big memory spike.
- [ ] **Non-UTF-8 pages still render.** Visit an older site that declares
      `charset=windows-1252` or `ISO-8859-1`.
      *Expected:* accented characters display correctly.

- [ ] **Certificate checks (automated, needs internet).** Run
      `cargo test --test live_tls -- --ignored`.
      *Expected:* 4 passed. These confirm that expired, self-signed, wrong-host, untrusted and
      (on macOS/Windows) revoked certificates are rejected, TLS 1.0/1.1 are refused, and
      HTTP/2 still works.

## I2P (needs a local I2P router on 127.0.0.1:4444)

- [ ] **(priority) Browsing in I2P mode.** Press `p` and load an `.i2p` site.
      *Expected:* it loads through the proxy.
- [ ] **(priority) HTTPS through I2P.** In I2P mode, load an `https://` site (via an outproxy,
      or an I2P site that offers HTTPS).
      *Expected:* it loads. This connection now uses rustls with TLS 1.2 minimum, and has not
      been tested against a real router.
- [ ] **(priority) Jump services.** In I2P mode, open an `.i2p` address your router doesn't
      know yet.
      *Expected:* "Requesting Jump Helper..." and the jump services are tried in turn.
      Clearweb sites with server errors never trigger a jump lookup.

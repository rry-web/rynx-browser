# Roadmap

Notes for future development: what's planned, in roughly the order it's worth doing, and ideas
being explored. The README's "Features to add" is the short public list; this is the longer
working one. Bugs found in testing come before all of it.

## Next

1. **Multi-key sequences** (`gg`, `gt`), now that link hints are done (`f`, and `;` for a new
   tab; numbered, so they can be said as well as typed).
2. **Homebrew tap.** A formula pointing at the release builds; then the AUR and others.
3. **Session restore (opt-in).** Reopen the last session's tabs at start.

## Later

- **Encryption at rest** for history, the cache and snapshots, with the same keychain key as
  cookies (`src/cookies.rs`). Weigh it the way cookie encryption was: it protects against other
  programs and copied files, not against malware running as the user.
- **A Windows sandbox.** Linux has Landlock and macOS Seatbelt; Windows has none yet. Candidates:
  AppContainer, or a restricted token with job objects.
- **Images, together:** SVG (rendered to pixels, e.g. with resvg), drawing images that are only
  partly on screen, and the larger copy a page offers in srcset on terminals that draw real
  pixels. (Sizes as the page sizes them, zoom, galleries side by side and a reading viewer are
  done.) Image work is shared, not something to hand over.
- **Captchas and cookies:** a form with a captcha image usually needs a session cookie, which
  rynx doesn't keep for a site until `C`; say so by the form's submit button, rather than letting
  the site answer "wrong captcha". (Found on an I2P imageboard, 10channel.i2p.)
- **Gemini and Gopher downloads**, and Gemini client certificates.
- **I2P's other uses** (`about:i2p` is the guide): send a magnet link straight to a local torrent
  client's web interface (I2PSnark, XD, qBittorrent) instead of copying it; and fixes from checking
  Java I2P's console, I2PSnark and SusiMail in rynx (TESTING.md's I2P-07).
- **Finer cookie control:** per cookie, and rules for third-party cookies.
- **History times in the local time zone.**
- **Site tools:** your own MCP servers usable on any page; sign-in (OAuth); servers asking
  questions while a tool runs.
- **Code-signed builds** for macOS (Apple Developer account, about $99 a year, plus
  notarization) and Windows (a certificate, or Azure Trusted Signing). A cost decision.
- **A Tor mode**, beside I2P mode: through a running Tor's SOCKS port, or arti (Tor in Rust)
  built in, with the same lock setting; Tor Browser's identity rather than Safari's. The only
  thing that hides your address from your network and sites on the clearweb.
- **A "new identity" key:** forget, at once, the session's cookies, Anubis passes and cached
  pages, before switching to something unrelated.
- **Reproducible builds**, so anyone can check a release was built from the published source
  (worth more while the builds aren't code-signed).
- **"Needs JavaScript" pages, next step:** when a page is only a JavaScript notice, show the
  article the page carries as data (`application/ld+json`, Next.js's `__NEXT_DATA__`, its
  description), in reader mode or in place of the notice. (Meta refreshes and simple script
  redirects are done.)
- **A built-in I2P router**, probably emissary (Rust) once it matures, so I2P needs nothing
  installed. Until then, better progress reports from older i2pd versions, whose console is slow
  to answer while starting.

## Won't do

- **Getting past JavaScript bot checks** such as Cloudflare's. rynx sends Safari's identity so it
  doesn't stand out, but doesn't fake a browser's environment to pass checks; since 0.3.1 it says
  what the check is and offers the Internet Archive's copy (`A`). Anubis is different: its proof
  of work is passed natively, as designed.
- **Running JavaScript.**

## Reticulum and NomadNet

Reticulum is an encrypted mesh network that runs over LoRa radio, packet radio, serial links,
ordinary networks and I2P; its pages are NomadNet's, written in Micron. Text pages on slow,
off-grid links suit rynx (and the glasses idea below).

1. **Micron rendering** (done, `src/micron.rs`): `.mu` pages drawn as NomadNet draws them, through
   rynx's renderer and its own markup (`data-rynx-indent`, `-align`, `-fg`/`-bg`, `-rule`). Not yet:
   images and partials are links, fields show but can't be sent, collapsible sections show open,
   page colours (`#!bg`, `#!fg`) are ignored. Other nodes' links become `nomadnet://<node>/path`.
2. **Pages through a local bridge** (done, `bridges/reticulum/`): a small Python program using
   Reticulum's reference software fetches NomadNet pages for `nomadnet://<node>/<path>` addresses
   (or NomadNet's own `<node>:/path`), with the X-Rynx header it requires; files come as
   downloads. MIT-licensed, beside Reticulum's own licence. Next: send a page's fields with the
   link that names them (as `field_<name>` request data, which the bridge already passes on), and
   images (`/media`) and partials.
3. **Native Reticulum**: speak it from rynx in Rust. Candidates: Leviculum (said to be
   functionally complete, tested against the Python version), reticulum-rs, nomadnet-rs. Check
   their maturity first; a Reticulum identity kept on disk needs care with the sandbox.

Messaging (LXMF: Sideband, NomadNet) stays with the programs made for it.

## Idea: rynx on monochrome AR glasses

A low-power single-board computer running headless Linux and rynx, shown on a monochrome glasses
display and driven by voice.

Why it fits: text suits a monochrome display; rynx marks links and the selection with underline
and reverse video rather than colour alone; there's no JavaScript engine or graphics stack, so a
small board copes. A 640×480 display with an 8×16 font is 80×30 cells, which works; shorter
displays (640×200 is about 12 rows) get tight.

Voice: a few dozen fixed commands is a small recognition job. Vosk or whisper.cpp with a fixed
phrase list runs on a Raspberry Pi 5-class CPU. An FPGA for inference adds design work, power and
board space for something a CPU can do, so it's a later optimisation (or a learning project), not
the base. A board with a built-in NPU is the easier step up if the CPU isn't enough.

What rynx would need, in order:

1. **Link hints** (done): "follow 12" instead of "next link" twenty times.
2. **A monochrome mode** (done): `colour = off`, or `NO_COLOR`; what colour marked is reversed.
3. **A control socket** (done): `--control-socket`, and glasses mode (`--glasses`) with a voice
   controller (`glasses/voice.py`, Vosk with a fixed phrase list), started together by
   `scripts/rynx-glasses`.
4. **Small-screen tuning:** reader mode by default; check every page and box at 80×30 and less.
5. **Few buttons** (done): a menu (`m`) of what can be done here, worked by number or by
   next/previous/choose/back, and push-to-talk (Space), so a remote or buttons on the glasses
   (set up as keys) and voice share one way of working.

Later, for the hardware: buttons on the glasses as a Linux keyboard (gpio-keys), a touch strip
whose swipes are next and previous, and a voice recogniser that builds on musl (Vosk's Python
packages are glibc only) for Alpine on a Raspberry Pi Zero 2 W class board.

Outside rynx: typing addresses and searches needs free dictation, which is harder than fixed
commands (bookmarks and history make it rarer); headless Linux still needs a terminal on the
display (the framebuffer console, or a small terminal emulator drawing straight to it); and the
computer and battery likely ride in a pocket, wired or wireless to the glasses.

First step (ready): the prototype on a laptop, `scripts/rynx-glasses` (glasses/README.md), or a
release's glasses package for others to try. Using it for a while shows whether browsing by voice
feels good, and what's missing, before buying hardware.
The compact layout is done (glasses mode: a title row and a status row), drawn in a screen of the
display's size set in `glasses.conf`, so layouts for other displays can be tried on a laptop. Then:
the voice controller on a Raspberry Pi, and checking every page and box at 80×30 and less.

## Process notes

- **Disk space.** Full checks build a lot: `target` reached 26 GB over a few weeks and filled the
  disk twice. `scripts/release.sh` refuses to start with under 20 GB free; `cargo clean` (and the
  same in `fuzz/`) frees the old builds.
- **Releasing** needs the person's go-ahead each time, and is run by them
  (`scripts/release.sh X.Y.Z`): it bumps the version, runs every check, pushes, waits for CI and
  tags; the tag's workflow builds, signs and publishes to GitHub and crates.io. A tag can't be
  moved once pushed, so a failed release moves on to the next version.
- **Commits:** check `git status` before committing everything; an editor's swap file
  (`.TESTING.md.swp`) once slipped in. `*.swp` is ignored now.
- **Testing:** TESTING.md maps every check to its test and lists the few still done by hand.
  New bugs get a test that fails without the fix, preferably a terminal test
  (`tests/terminal/`) when the bug is about keys or the screen.

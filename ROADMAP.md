# Roadmap

Notes for future development: what's planned, in roughly the order it's worth doing, and ideas
being explored. The README's "Features to add" is the short public list; this is the longer
working one. Bugs found in testing come before all of it.

## Next

1. **The system-proxy gap.** A proxy set in the system's settings bypasses the check that refuses
   public names resolving to local addresses (see `src/dns.rs`). It's the one known hole in the
   network protections, and small.
2. **Link hints.** `f` (unbound now) labels every visible link with a short code; typing it
   follows the link. `F` already opens the selected link in a new tab, so hints in a new tab need
   another key or a modifier. This finishes the Vim-style keys, and it's essential for voice control (below). Multi-key
   sequences (`gg`, `gt`) would come with the same input handling.
3. **Homebrew tap.** A formula pointing at the release builds; then the AUR and others.
4. **Session restore (opt-in).** Reopen the last session's tabs at start.

## Later

- **Encryption at rest** for history, the cache and snapshots, with the same keychain key as
  cookies (`src/cookies.rs`). Weigh it the way cookie encryption was: it protects against other
  programs and copied files, not against malware running as the user.
- **A Windows sandbox.** Linux has Landlock and macOS Seatbelt; Windows has none yet. Candidates:
  AppContainer, or a restricted token with job objects.
- **Images, together:** SVG (rendered to pixels, e.g. with resvg), and drawing images that are
  only partly on screen. Image work is shared, not something to hand over.
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
- **A built-in I2P router**, probably emissary (Rust) once it matures, so I2P needs nothing
  installed. Until then, better progress reports from older i2pd versions, whose console is slow
  to answer while starting.

## Won't do

- **Getting past JavaScript bot checks** such as Cloudflare's. rynx introduces itself honestly
  and doesn't disguise itself; since 0.3.1 it says what the check is and offers the Internet
  Archive's copy (`A`). Anubis is different: its proof of work is passed natively, as designed.
- **Running JavaScript.**

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

1. **Link hints** (above): "follow 12" instead of "next link" twenty times.
2. **A monochrome mode:** no colour, meaning carried by bold, underline and reverse; and honour
   the `NO_COLOR` convention.
3. **A control socket** taking action names (`next_tab`, `open_in_new_tab`... the names in
   `src/keys.rs`), so a voice program needn't know which keys are bound. Until then, run rynx in
   tmux and send keys with `tmux send-keys`, which needs no change to rynx.
4. **Small-screen tuning:** reader mode by default; check every page and box at 80×30 and less.

Outside rynx: typing addresses and searches needs free dictation, which is harder than fixed
commands (bookmarks and history make it rarer); headless Linux still needs a terminal on the
display (the framebuffer console, or a small terminal emulator drawing straight to it); and the
computer and battery likely ride in a pocket, wired or wireless to the glasses.

First step: a prototype on a laptop. An 80×30 terminal with colour off, a speech recogniser with
a fixed command list, and a short script sending keys through tmux. That shows whether browsing by
voice feels good before buying hardware.

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

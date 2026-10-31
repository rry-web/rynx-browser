# rynx on glasses: glasses mode and voice control

rynx can be driven by voice, for monochrome AR glasses: a small Linux computer running rynx in a
terminal on the glasses' display, and a speech recogniser turning what you say into commands.
Everything here also runs on a laptop, to try it before any hardware.

## Trying it on a Mac (or Linux)

```sh
cargo build --release
scripts/rynx-glasses
```

The first run sets up the voice controller's Python packages (`vosk`, `sounddevice`) in
`~/.cache/rynx/glasses-venv` and downloads Vosk's small English speech model (40 MB, checked
against its known SHA-256). Then rynx starts in glasses mode in that terminal, and the voice
controller listens in the background. macOS asks once whether the terminal app may use the
microphone.

Say **"links"**: every link on screen gets a number. Say **"follow three"** (or just
**"three"** while they're numbered). What was heard shows in rynx's status line; the voice
controller's log is `~/.cache/rynx/voice.log`.

No microphone, or just checking the commands? `scripts/rynx-glasses --text` starts rynx alone,
and in a second terminal `python3 glasses/voice.py --text` takes what you'd say, typed.

## What to say

| Say | Does |
| :--- | :--- |
| links, show links | Number the links and fields on screen |
| follow *N*, open *N*, *N* (while numbered) | Open link *N* |
| open *N* in new tab, links in new tab | The same, in a new tab |
| down, up, page down, page up, top, bottom | Scroll |
| back, forward, reload | History |
| go to *(pause)* wikipedia dot org | Open an address ("dot" and "slash" are typed) |
| search *(pause)* terminal browsers | Search for the words |
| find *(pause)* potato | Find on the page |
| type *(pause)* words, submit | Type into the field chosen with "links" and its number; send the form |
| next link, previous link, select | Step through links, open the selected one |
| new tab, close tab, next tab, previous tab, reopen tab | Tabs |
| read, stop | Read the page aloud from the top of the screen; stop reading |
| where am i | Say the page's title and the status line |
| cancel | As Esc |
| reader mode, bookmarks, bookmark this, history, load images, show keys | As their keys |
| quit browser | Quit rynx |
| help | A short reminder, spoken |

Commands said one after another without a pause ("links follow three") are taken in turn. A
number on its own only follows a link while the links are numbered, so a stray word heard as
"two" can't follow one. While rynx's answer is being read aloud, only "stop" is listened for,
since the microphone hears the voice too.

## Glasses mode

`rynx --glasses` is rynx as usual, plus:

- **No colour**: what colour marked (the selected link, matches, modes) is drawn in reverse video,
  for monochrome displays. (`colour = off` in settings, or `NO_COLOR`, does this in ordinary mode.)
- **A compact layout**: one row at the top for the mode and the page's title (or the address
  being typed), the page, and one row at the bottom for the status line and where the selected
  link goes. The tab bar, address bar and border took eight rows; at 80×30 the page gets 28
  rows instead of 22.
- **Reader mode** in every tab: only a page's main text.
- **An 80×30 screen**, asked of the terminal (what a 640×480 display with an 8×16 font shows);
  terminals that don't allow it keep their size.
- **The control socket**, below.

## The control socket

`rynx --control-socket` (which `--glasses` includes) lets programs on this computer drive rynx by
naming actions, so they needn't know which keys are bound. It's a Unix socket at
`control/rynx.sock` in rynx's settings folder (`~/Library/Application Support/rynx-browser` on
macOS, `~/.config/rynx-browser` on Linux), in a folder only you can open, or wherever
`RYNX_CONTROL_SOCKET` says. One command per line; one line of JSON back for each,
`{"ok": true, "message": "..."}`:

| Command | Does |
| :--- | :--- |
| an action from `keys.conf` | What its key does (`scroll_down`, `back`, `hints`, `next_tab`...); `actions` lists them |
| `follow 12`, `follow_in_new_tab 12` | Open the link `f` (or `;`) numbers 12 |
| `open words` | An address, or words to search for, as typed into the address bar |
| `find words` | Find on the page, and jump to the first match |
| `type text` | Typed into the field being typed in (or the address bar, or find); refused while browsing, where each letter would be a key |
| `key Esc` | One key, as pressed (written as in `keys.conf`) |
| `status` | `title`, `address`, the status line (`message`), `mode`, `tab` and `tabs` |
| `read` | `text`: the page from the top of the screen down, for reading aloud |
| `heard words` | Show "Heard: words" in the status line |

```sh
echo "open wikipedia.org" | nc -U ~/Library/Application\ Support/rynx-browser/control/rynx.sock
```

Any program running as you can use the socket, and read the page with `read`; such a program
could already type into the terminal rynx runs in. It's only there while rynx runs with one of
those options. Windows has no control socket yet.

## On the glasses

The plan (ROADMAP.md has more): a Raspberry Pi 5-class board with headless Linux, rynx in a
terminal drawn straight to the display (the framebuffer console, or a small terminal emulator),
`voice.py` with a USB or Bluetooth microphone, and `espeak-ng` for reading aloud. Vosk's small
model runs comfortably on such a board's CPU.

## Tests

`python3 glasses/test_voice.py` checks the phrases and their commands. With Vosk installed and
`VOSK_MODEL` naming the model's folder, on macOS it also speaks commands with `say` and checks
they're recognised. rynx's side is `tests/terminal/glasses.rs`.

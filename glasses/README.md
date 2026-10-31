# rynx on glasses: glasses mode and voice control

rynx can be driven by voice, for monochrome AR glasses: a small Linux computer running rynx in a
terminal on the glasses' display, and a speech recogniser turning what you say into commands.
Everything here also runs on a laptop, to try it before any hardware.

## Trying it on a Mac (or Linux)

From a release: download `rynx-glasses-<version>-<platform>.tar.gz` (macOS and Linux), extract
it, and run `./rynx-glasses` in it. Its README.md (this folder's [PACKAGE.md](PACKAGE.md)) is
written for people trying it out. From the source:

```sh
cargo build --release
scripts/rynx-glasses
```

The first run sets up the voice controller's Python packages (`vosk`, `sounddevice`) in
`~/.cache/rynx/glasses-venv` and downloads Vosk's small English speech model (40 MB, checked
against its known SHA-256). Then rynx starts in glasses mode in that terminal, and the voice
controller runs in the background. macOS asks once whether the terminal app may use the
microphone.

Voice is listened to only when you ask, two ways:

- **Hold to talk:** hold **Right Option** (Right Alt on Linux) while you speak, and let go. The
  voice controller reads that key from the keyboard itself, since a terminal says when keys are
  pressed but never when they're let go; it looks at that one key and keeps nothing else typed.
  macOS asks once to let the terminal app see keys (Accessibility and Input Monitoring, in
  Privacy & Security; restart the terminal after). On Linux it needs read access to
  `/dev/input` (the `input` group), which is also how buttons on the glasses would arrive.
  `--hold-key right_ctrl` (or `right_shift`, `f13`, `KEY_...` on Linux) picks another key.
- **Press to talk:** press **Space** (in a field, where Space types, **Ctrl+Space** or **F2**; or
  choose Talk in the menu). Listening stops after one command, or after 8 seconds of nothing.

The top row shows **TALK** while it's listening. Nothing said in passing becomes a command, the
microphone doesn't hear rynx reading aloud, and on a small computer the recogniser only works when
asked. `scripts/rynx-glasses --always-listening` listens all the time instead.

Hold the key and say **"links"**: every link on screen gets a number. Say **"three"** (or
"follow three"). Or say **"menu"** and choose from what it lists. What was heard shows in rynx's
status line; the voice controller's log is `~/.cache/rynx/voice.log`.

### Typing by voice

Say **"search pancake recipes"** in one breath (or "search", then the words): the words are typed
into the address bar, and rynx waits. Say **"send"** to search, **"again"** to say them again, or
**"cancel"**. Likewise **"go to wikipedia dot org"**, **"find potato"** on the page, and
**"fill flour and sugar"**, which types into the page's search box (the first text field on
screen, else on the page), with no need to number the links and choose it. In a field you've
chosen, whatever you say is typed. Nothing is sent until you say "send".

## The menu

`m` (or saying "menu") opens a short numbered list of what can be done here: for the selected
link (open it, in a new tab, the next or previous link), for a field (use it), or for the page
(number the links, go to an address, back, forward); and under **Page...** (find, reader mode,
reload, bookmarks, history, images, help) and **Tabs...** (new, next, previous, close, reopen,
quit), and **Talk**. Choose by number (typed, or said), or step through it and choose: Tab or
Down, Shift+Tab or Up, Enter; Backspace goes back to the main menu, Esc closes it.

The keys bound to `next_link`, `previous_link`, `open` and `back` work the menu too, so four
buttons (a remote, or buttons on the glasses, set up as keys in `keys.conf`) reach everything:
next, previous, choose, back, with `m` and Space on two more.

`scripts/rynx-glasses --layout 640x200` tries another display's size (see "Layouts" below).
No microphone, or just checking the commands? `scripts/rynx-glasses --text` starts rynx alone,
and in a second terminal `python3 glasses/voice.py --text` takes what you'd say, typed.

## What to say

| Say | Does |
| :--- | :--- |
| menu, options | The menu |
| choose *N*, option *N*, *N* (while the menu is open) | Menu item *N* |
| links, show links | Number the links and fields on screen |
| follow *N*, open *N*, *N* (while numbered) | Open link *N* |
| open *N* in new tab, links in new tab | The same, in a new tab |
| down, up, page down, page up, top, bottom | Scroll |
| back, forward, reload | History |
| go to wikipedia dot org | Type an address ("dot" and "slash" are typed) |
| search terminal browsers | Type a search |
| fill *words*, search here *words* | Type into the page's text field |
| find potato | Find on the page |
| type *words* | Type into the field being typed in |
| send, again, cancel | After words are typed: send them, say them again, or stop |
| next, previous, select (or next link...) | Step through links (or the menu), open the selected one |
| new tab, close tab, next tab, previous tab, reopen tab | Tabs |
| read, stop | Read the page aloud from the top of the screen; stop reading |
| where am i | Say the page's title and the status line |
| cancel | As Esc |
| reader mode, bookmarks, bookmark this, history, load images, show keys | As their keys |
| quit browser | Quit rynx |
| help | A short reminder, spoken |

Commands said one after another without a pause ("links follow three") are taken in turn. A
number on its own only follows a link while the links are numbered, or chooses from the menu
while it's open, so a stray word heard as "two" can't do either. While rynx's answer is being read aloud, only "stop" is listened for,
since the microphone hears the voice too.

## Glasses mode

`rynx --glasses` is rynx as usual, plus:

- **A screen the display's size**: 80×30 by default (what a 640×480 display with an 8×16 font
  shows), drawn in the window's top left corner whatever the window's size, with a line along
  its edge. rynx also asks the terminal for a window that size (Terminal does it; iTerm2 and
  others may not, which doesn't matter).
- **A compact layout**: one row at the top for the mode and the page's title (or the address
  being typed), the page, and one row at the bottom for the status line and where the selected
  link goes. The tab bar, address bar and border took eight rows; at 80×30 the page gets 28
  rows instead of 22.
- **No colour**: what colour marked (the selected link, matches, modes) is drawn in reverse video,
  for monochrome displays. (`colour = off` in settings, or `NO_COLOR`, does this in ordinary mode.)
- **Reader mode** in every tab: only a page's main text.
- **The control socket**, below.

## Layouts for other displays

The screen and layout are in `glasses.conf` in rynx's settings folder
(`~/Library/Application Support/rynx-browser` on macOS, `~/.config/rynx-browser` on Linux),
written with the defaults the first time glasses mode starts. Change it and restart rynx.

| Line | Does |
| :--- | :--- |
| `screen = 80x30` | Columns and rows of text |
| `screen = 640x480 pixels` and `font = 8x16` | The same, worked out from the display's pixels and the font's size |
| `screen = window` | The whole terminal window |
| `title_row = on` | The top row: the mode and the page's title |
| `status_row = on` | The bottom row: the status line, where what was heard shows |
| `margin = 1` | Blank columns either side of the page |
| `colour = off` | Off: reverse video where colour was, for monochrome displays |
| `reader_mode = on` | Every tab in reader mode |

To compare layouts without editing it, name another file: `rynx --glasses-layout FILE`, or
`scripts/rynx-glasses --layout NAME` for the ones in [layouts/](layouts/):

| Layout | Screen | For |
| :--- | :--- | :--- |
| `640x480` | 80×30 | 640×480 displays (the default), like the Vuzix Z100's |
| `640x350` | 80×21 | Wide, short displays, like the size reported for the Even Realities G2's |
| `640x200` | 80×12, no title row | A strip, like the Even Realities G1's |
| `304x256` | 38×16, no margins | Small displays, like ActiveLook's |
| `large-font` | 53×20 | 640×480 with a 12×24 font, easier to read at a glance |
| `window` | The whole window, in colour | The compact layout on an ordinary screen |

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
| `choose 3` | Item 3 of the open menu |
| `talk on`, `talk off` | Push-to-talk on (as Space does), or off quietly (once a command is heard); `status` says `listening` |
| `begin open`, `begin find`, `begin fill` | Start typing an address or search, words to find, or into the page's text field (emptied); then `type`, and `key Enter` or `key Esc` |
| `erase 5` | Take back the last 5 characters typed |

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

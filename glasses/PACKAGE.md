# rynx for glasses: trying it out

rynx is a terminal web browser. This package starts it in **glasses mode**: a small screen
(80×30, what a 640×480 glasses display shows), no colour, pages in reader mode, and voice
control. A laptop stands in for the glasses: the screen is drawn in the top left corner of your
terminal window.

Speech is recognised on this computer (Vosk, a small offline model); nothing you say is sent
anywhere.

## Needs

- **macOS** (Apple Silicon or Intel) or **Linux**, and a terminal.
- **Python 3** for voice control. On macOS, `python3` in Terminal offers to install it (or run
  `xcode-select --install`). On Linux, your distribution's `python3` with `python3-venv`, plus
  PortAudio for the microphone (`libportaudio2` on Debian, Ubuntu and Raspberry Pi OS) and
  `espeak-ng` to hear answers aloud.
- A microphone. Headphones help, so the microphone doesn't hear rynx reading aloud.

## Starting

```sh
tar xzf rynx-glasses-*.tar.gz
cd rynx-glasses-*/
./rynx-glasses
```

The first start takes a minute or two:

1. **macOS only:** if the download was quarantined, it asks whether to allow this copy of rynx to
   run (it isn't code-signed). Answer `y`.
2. It sets up the voice controller's Python packages in `~/.cache/rynx/glasses-venv`.
3. It downloads the speech model (40 MB, checked against its known checksum).
4. rynx starts. macOS asks once whether the terminal may use the microphone: allow it.

## Using it

Voice is **push-to-talk**: press **Space**, say one command, and listening stops once it's heard.
The top row shows **TALK** while it's listening.

| Say (after Space) | Does |
| :--- | :--- |
| menu | A numbered list of what you can do here |
| *a number*, choose *N* | That item of the menu |
| links | Number the links on screen |
| *a number*, follow *N* | Open that link |
| go to *(pause)* wikipedia dot org | Open an address |
| search *(pause)* pancake recipes | Search the web |
| down, up, back, forward | Scroll, and history |
| next, previous, select | Step through links, and open the one selected |
| read, stop | Read the page aloud, and stop |
| help | A short spoken reminder |

The keyboard does everything too. The few keys that reach everything:

| Key | Does |
| :--- | :--- |
| `m` | The menu: then a number, or Tab/arrows to step and Enter to choose; Backspace goes back a level, Esc closes |
| `f` | Number the links, then type a number |
| `Space` | Talk (one voice command) |
| Up, Down, `PageUp`, `PageDown` | Scroll |
| `Backspace` | Back |
| `e` | Type an address or search |
| `?` | All the keys |
| `q` | Quit |

### Other screen sizes

`./rynx-glasses --layout NAME` tries another display's size: `640x350`, `640x200` (a strip,
12 rows), `304x256` (38 columns), `large-font`, or `window` (the whole terminal, in colour). They
are the files in `layouts/`. The default layout is `glasses.conf` in rynx's settings folder
(`~/Library/Application Support/rynx-browser` on macOS, `~/.config/rynx-browser` on Linux); edit
it and restart to try your own.

### Without a microphone

`./rynx-glasses --text`, then in a second terminal `python3 voice.py --text`, and type what you'd
say. `./rynx-glasses --always-listening` listens all the time instead of after Space.

## Telling us how it went

Useful to hear: what you tried to do, what worked, what was awkward, and what you said that
wasn't understood. What the voice controller heard is in `~/.cache/rynx/voice.log` (it's on
your computer only; look through it before sending any of it).

## Checking the download

The release's `SHA256SUMS` lists this archive, and is signed with rynx's release key
(`SHA256SUMS.sig`). See "Verifying" in the release notes, or:

```sh
shasum -a 256 -c SHA256SUMS --ignore-missing      # Linux: sha256sum
gh attestation verify rynx-glasses-*.tar.gz --repo rry-web/rynx-browser
```

## Removing it

Delete this folder, `~/.cache/rynx` (the voice controller and speech model), and, if you don't
use rynx otherwise, its settings folder (above).

`GUIDE.md` has everything about glasses mode and voice control; `LICENSE.txt` is rynx's licence.

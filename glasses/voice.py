#!/usr/bin/env python3
"""Voice control for rynx: say "links", then "follow twelve", and rynx follows link 12.

Speech is recognised on this computer with Vosk (nothing is sent anywhere), listening for a fixed
list of commands, which a small model recognises quickly even on a Raspberry Pi. After "search",
"go to", "find" or "type", the next thing said is taken as free words. Commands go to rynx's
control socket (rynx --glasses, or --control-socket), as action names, so keys.conf doesn't
matter.

    python3 glasses/voice.py             listen to the microphone
    python3 glasses/voice.py --push-to-talk
                                         only after the talk key (Space in rynx, or the menu's
                                         Talk), for one command
    python3 glasses/voice.py --text      type what you'd say instead (no microphone or Vosk needed)
    python3 glasses/voice.py --wav F     recognise a recording (16 kHz mono WAV), for testing

Needs: pip install vosk sounddevice (scripts/rynx-glasses sets it all up), and Vosk's small
English model, which --download fetches (40 MB, checked against its known SHA-256).
"""

import argparse
import hashlib
import json
import os
import queue
import shutil
import socket
import subprocess
import sys
import threading
import time
import zipfile
from pathlib import Path

MODEL_NAME = "vosk-model-small-en-us-0.15"
MODEL_URL = f"https://alphacephei.com/vosk/models/{MODEL_NAME}.zip"
MODEL_SHA256 = "30f26242c4eb449f948e42cb302dd7a686cb29a3423a8367f99ff41780942498"
SAMPLE_RATE = 16000

# What's said, and the rynx command it becomes. "N" stands for a number.
PHRASES = {
    "down": "half_page_down",
    "scroll down": "half_page_down",
    "up": "half_page_up",
    "scroll up": "half_page_up",
    "page down": "page_down",
    "next page": "page_down",
    "page up": "page_up",
    "previous page": "page_up",
    "line down": "scroll_down",
    "line up": "scroll_up",
    "top": "top",
    "go to top": "top",
    "bottom": "bottom",
    "go to bottom": "bottom",
    "links": "hints",
    "show links": "hints",
    "numbers": "hints",
    "links in new tab": "hints_in_new_tab",
    "new tab links": "hints_in_new_tab",
    "follow N": "follow N",
    "open N": "follow N",
    "link N": "follow N",
    "number N": "follow N",
    "N": "follow N",
    "open N in new tab": "follow_in_new_tab N",
    "follow N in new tab": "follow_in_new_tab N",
    "next link": "next_link",
    "previous link": "previous_link",
    "next": "next_link",
    "previous": "previous_link",
    "menu": "menu",
    "show menu": "menu",
    "options": "menu",
    "choose N": "choose N",
    "option N": "choose N",
    "item N": "choose N",
    "select": "open",
    "click": "open",
    "open it": "open",
    "enter": "key Enter",
    "submit": "key Enter",
    "send": "key Enter",
    "back": "back",
    "go back": "back",
    "forward": "forward",
    "go forward": "forward",
    "reload": "reload",
    "refresh": "reload",
    "cancel": "key Esc",
    "escape": "key Esc",
    "never mind": "key Esc",
    "new tab": "new_tab",
    "close tab": "close_tab",
    "next tab": "next_tab",
    "previous tab": "previous_tab",
    "reopen tab": "reopen_tab",
    "reader": "reader",
    "reader mode": "reader",
    "bookmark this": "bookmark",
    "bookmarks": "bookmarks",
    "show bookmarks": "bookmarks",
    "history": "history",
    "next match": "next_match",
    "previous match": "previous_match",
    "load images": "load_images",
    "switch network": "i2p",
    "show keys": "help",
    "quit browser": "quit",
    # Handled here rather than by rynx: free words come next (or straight after, in one breath)
    "search": "@dictate open",
    "search for": "@dictate open",
    "go to": "@dictate open",
    "look up": "@dictate open",
    "find": "@dictate find",
    "type": "@dictate type",
    "fill": "@dictate fill",
    "fill in": "@dictate fill",
    "search here": "@dictate fill",
    "again": "@again",
    "try again": "@again",
    "read": "@read",
    "read page": "@read",
    "read this": "@read",
    "stop": "@stop",
    "stop reading": "@stop",
    "quiet": "@stop",
    "where am i": "@status",
    "status": "@status",
    "help": "@help",
}

ONES = "zero one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen".split()
TENS = {"twenty": 20, "thirty": 30, "forty": 40, "fifty": 50, "sixty": 60, "seventy": 70, "eighty": 80, "ninety": 90}
# Words the recogniser hears for numbers that sound alike
SOUNDALIKES = {"to": "two", "too": "two", "for": "four", "won": "one", "ate": "eight", "tree": "three"}


def number(words):
    """The number some words say (up to ninety nine, or digits), or None"""
    words = [SOUNDALIKES.get(w, w) for w in words]
    if len(words) == 1 and words[0].isdigit():
        return int(words[0])
    if len(words) == 1 and words[0] in ONES:
        return ONES.index(words[0])
    if len(words) in (1, 2) and words[0] in TENS:
        if len(words) == 1:
            return TENS[words[0]]
        if words[1] in ONES[1:10]:
            return TENS[words[0]] + ONES.index(words[1])
    return None


def understand(text):
    """What a phrase means: a rynx command, an @command handled here, or None"""
    # [unk] is the recogniser's "something else was said": left out, so "[unk] search" is search
    words = [w for w in text.lower().replace("-", " ").split() if w != "[unk]"]
    if not words:
        return None
    if " ".join(words) in PHRASES:
        return PHRASES[" ".join(words)]
    # Phrases with a number: try every way of splitting off the words that say it
    for phrase, command in PHRASES.items():
        if "N" not in phrase.split():
            continue
        before, _, after = phrase.partition("N")
        before, after = before.split(), after.split()
        if words[: len(before)] != before or (after and words[-len(after):] != after):
            continue
        n = number(words[len(before): len(words) - len(after)])
        if n is not None and n > 0:
            return command.replace("N", str(n))
    return None


def split_commands(text):
    """Commands said one after another without a pause ("links follow three"), longest phrase
    first at each point; None if any part isn't a command"""
    words = [w for w in text.lower().split() if w != "[unk]"]
    parts, start = [], 0
    while start < len(words):
        end = next((e for e in range(len(words), start, -1) if understand(" ".join(words[start:e]))), None)
        if end is None:
            return None
        parts.append(" ".join(words[start:end]))
        start = end
    return parts


def spoken_numbers():
    """One to ninety nine, as said"""
    said = ONES[1:]
    for tens in TENS:
        said += [tens] + [f"{tens} {one}" for one in ONES[1:10]]
    return said


def grammar():
    """Every phrase the commands are said with, numbers spelled out: whole phrases, so the
    recogniser knows "go" comes before "back" or "to" (given single words it heard "go to" as
    "[unk] two")"""
    phrases = []
    for phrase in PHRASES:
        if "N" in phrase.split():
            phrases += [phrase.replace("N", n) for n in spoken_numbers()]
        else:
            phrases.append(phrase)
    return phrases


def address_words(text):
    """Free words said after "go to" or "search", as typed: "wikipedia dot org" is wikipedia.org"""
    words = text.lower().split()
    if "dot" in words:
        joined = "".join("." if w == "dot" else ("/" if w == "slash" else w) for w in words)
        return joined
    return text


def default_socket():
    """Where rynx makes its control socket: RYNX_CONTROL_SOCKET, or in its settings folder"""
    if os.environ.get("RYNX_CONTROL_SOCKET"):
        return Path(os.environ["RYNX_CONTROL_SOCKET"])
    if sys.platform == "darwin":
        config = Path.home() / "Library" / "Application Support" / "rynx-browser"
    else:
        config = Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config") / "rynx-browser"
    return config / "control" / "rynx.sock"


class Rynx:
    """The control socket, connected again whenever rynx restarts"""

    def __init__(self, path, log):
        self.path = path
        self.log = log
        self.stream = None

    def send(self, command):
        for attempt in range(2):
            try:
                if self.stream is None:
                    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                    sock.settimeout(15)
                    sock.connect(str(self.path))
                    self.stream = sock.makefile("rwb")
                self.stream.write(command.encode() + b"\n")
                self.stream.flush()
                line = self.stream.readline()
                if not line:
                    raise ConnectionError("rynx closed the socket")
                return json.loads(line)
            except (OSError, ConnectionError, ValueError) as e:
                self.stream = None
                if attempt == 1:
                    return {"ok": False, "message": f"can't reach rynx at {self.path}: {e}"}
        return {"ok": False, "message": "unreachable"}

    def wait(self, seconds=30):
        """Wait for rynx to make its socket"""
        deadline = time.time() + seconds
        while time.time() < deadline:
            if self.send("status").get("ok"):
                return True
            time.sleep(0.5)
        return False


class Speaker:
    """Speaking aloud with the system's voice (say on macOS, espeak-ng or espeak elsewhere)"""

    def __init__(self, enabled):
        program = "say" if sys.platform == "darwin" else (shutil.which("espeak-ng") or shutil.which("espeak"))
        self.program = program if enabled and program and shutil.which(program) else None
        self.process = None

    def say(self, text):
        self.stop()
        if self.program and text.strip():
            self.process = subprocess.Popen([self.program, text[:6000]], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    def speaking(self):
        return self.process is not None and self.process.poll() is None

    def stop(self):
        if self.speaking():
            self.process.terminate()
        self.process = None


# rynx's modes that type text: there, what's said is typed
TYPING_MODES = {"Editing", "Search", "FormEdit"}

PROMPTS = {"open": "Where to?", "find": "Find what?", "fill": "Type what?", "type": "Type what?"}


def dictation_in(text):
    """Free words said straight after search, go to, find, type or fill, in one breath: where
    they go and the words, or None"""
    words = text.split()
    for n in (2, 1):
        lead = " ".join(words[:n]).lower()
        meaning = PHRASES.get(lead, "")
        if meaning.startswith("@dictate ") and len(words) > n:
            return meaning.split()[1], " ".join(words[n:])
    return None


class Controller:
    """Turns what was heard into commands, keeping track of whether free words are expected next,
    and of words just typed, which wait for send, again or cancel"""

    def __init__(self, rynx, speaker, log):
        self.rynx = rynx
        self.speaker = speaker
        self.log = log
        self.dictating = None  # where the next free words go: "open", "find", "fill" or "type"
        self.typed = None  # what was just typed, until send, again or cancel

    def utterance(self, command_text, free_text):
        """One thing said, as both recognisers heard it (push-to-talk and hold-to-talk): the
        fixed commands', and the free words', for "search pancake recipes" in one breath"""
        if self.dictating:
            return self.heard(free_text or command_text)
        found = dictation_in(free_text) if self.typed is None else None
        if found and understand(command_text) not in ("@dictate " + found[0], None):
            found = None
        if found:
            target, words = found
            if not self.start_dictation(target, command_text or target, quiet=True):
                return True
            return self.heard(words)
        return self.heard(command_text if command_text.replace("[unk]", "").strip() else free_text)

    def heard(self, text):
        """Something was heard. Returns False once rynx has quit."""
        text = text.strip()
        if not text or text == "[unk]":
            return True
        if self.dictating:
            return self.dictated(text)
        # Words typed wait for send, again or cancel; another command goes on as usual, and
        # anything else is ignored (it isn't typed)
        if self.typed is not None:
            if understand(text) is None and not split_commands(text):
                self.rynx.send(f'heard {text}: say send, again, or cancel for "{self.typed}"')
                return True
            typed, self.typed = self.typed, None
            if understand(text) == "@again":
                self.rynx.send(f"erase {len(typed)}")
                self.dictating = "type"
                self.rynx.send("heard again: say it now")
                self.speaker.say("Say it again")
                return True
        # Several commands without a pause between them: each in turn
        parts = split_commands(text) if understand(text) is None else None
        if parts and len(parts) > 1:
            return all(self.heard(part) for part in parts)
        # While speaking, only "stop" is listened for: the microphone hears the voice too
        meaning = understand(text)
        if self.speaker.speaking() and meaning != "@stop":
            return True
        # A number on its own follows a link while the links are numbered (after "links"), or
        # chooses from the menu while it's open; otherwise a stray "to", heard as two, would act
        if meaning and meaning.startswith("follow") and number(text.replace("[unk]", "").split()) is not None:
            mode = self.rynx.send("status").get("mode")
            meaning = {"Hints": meaning, "Menu": meaning.replace("follow", "choose")}.get(mode)
        if meaning is None or meaning == "@again":
            self.log(f"not a command: {text}")
            self.rynx.send(f"heard {text} (not a command; say help)")
            return True
        self.log(f"{text} -> {meaning}")
        self.rynx.send(f"heard {text}")
        if meaning.startswith("@dictate "):
            self.start_dictation(meaning.split()[1], text)
            return True
        if meaning == "@stop":
            self.speaker.stop()
            return True
        if meaning == "@read":
            answer = self.rynx.send("read")
            self.speaker.say(answer.get("text") or "Nothing to read")
            return True
        if meaning == "@status":
            answer = self.rynx.send("status")
            self.speaker.say(f"{answer.get('title') or 'No title'}. {answer.get('message', '')}")
            return True
        if meaning == "@help":
            self.speaker.say("Say links, then a number. Or menu, down, up, back, search, go to, fill, find, read, next tab.")
            return True
        return self.run(meaning)

    def start_dictation(self, target, said, quiet=False):
        """Get ready for free words: the address bar, find, or the page's text field opened (type
        goes to whatever is being typed in). False if there's nowhere to type."""
        if target == "type":
            if self.rynx.send("status").get("mode") not in TYPING_MODES:
                return self.cannot("Not typing into a field: say fill, or links and the field's number")
        else:
            answer = self.rynx.send(f"begin {target}")
            if not answer.get("ok"):
                return self.cannot(answer.get("message", "That didn't work"))
        self.dictating = target
        if not quiet:
            self.rynx.send(f"heard {said}: {PROMPTS[target]} (say it now)")
            self.speaker.say(PROMPTS[target])
        return True

    def cannot(self, message):
        self.log(f"  {message}")
        self.rynx.send(f"heard {message}")
        self.speaker.say(message)
        return False

    def dictated(self, text):
        """Free words, typed where they go; then send, again or cancel"""
        target, self.dictating = self.dictating, None
        if understand(text) == "key Esc":
            self.log(f"{target}: cancelled")
            return self.run("key Esc")
        words = address_words(text) if target == "open" else text
        self.log(f"{target}: {words}")
        answer = self.rynx.send(f"type {words}")
        if not answer.get("ok"):
            self.cannot(answer.get("message", "That couldn't be typed"))
            return True
        self.typed = words
        self.rynx.send(f'heard "{words}": say send, again, or cancel')
        return True

    def run(self, command):
        answer = self.rynx.send(command)
        self.log(f"  {json.dumps(answer)[:200]}")
        if not answer.get("ok"):
            self.speaker.say(answer.get("message", "That didn't work"))
        return not command.endswith("quit") or not answer.get("ok")


def ensure_model(path, download):
    if (path / "am").is_dir():
        return path
    if not download:
        sys.exit(f"No speech model at {path}: run with --download to fetch it (40 MB), or give --model")
    import urllib.request

    path.parent.mkdir(parents=True, exist_ok=True)
    archive = path.parent / f"{MODEL_NAME}.zip"
    print(f"Downloading the speech model ({MODEL_URL})...", file=sys.stderr)
    urllib.request.urlretrieve(MODEL_URL, archive)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    if digest != MODEL_SHA256:
        archive.unlink()
        sys.exit(f"The downloaded model isn't the expected one (SHA-256 {digest}); not used")
    with zipfile.ZipFile(archive) as z:
        # Only the model's own folder, nothing outside it
        for name in z.namelist():
            if not name.startswith(f"{MODEL_NAME}/") or ".." in Path(name).parts:
                sys.exit(f"Unexpected file in the model archive: {name}")
        z.extractall(path.parent)
    archive.unlink()
    extracted = path.parent / MODEL_NAME
    if extracted != path:
        extracted.rename(path)
    return path


def recognisers(model_path):
    import vosk

    vosk.SetLogLevel(-1)
    model = vosk.Model(str(model_path))
    commands = vosk.KaldiRecognizer(model, SAMPLE_RATE, json.dumps(grammar() + ["[unk]"]))
    free = vosk.KaldiRecognizer(model, SAMPLE_RATE)
    return commands, free


class HoldKey:
    """A key held down to talk, read from the keyboard itself rather than through rynx's terminal,
    which says when keys are pressed but never when they're let go. Only that key is looked at:
    nothing else typed is kept or written anywhere."""

    # Friendly names, as pynput (macOS, X11) and evdev (Linux) call them
    NAMES = {
        "right_option": ("alt_r", "KEY_RIGHTALT"),
        "right_alt": ("alt_r", "KEY_RIGHTALT"),
        "right_ctrl": ("ctrl_r", "KEY_RIGHTCTRL"),
        "right_shift": ("shift_r", "KEY_RIGHTSHIFT"),
        "right_cmd": ("cmd_r", "KEY_RIGHTMETA"),
    }

    def __init__(self, name, log):
        self.name = name
        self.down = False
        pynput_name, evdev_name = self.NAMES.get(name, (name, name.upper()))
        if sys.platform != "darwin" and self.evdev(evdev_name, log):
            return
        self.pynput(pynput_name, log)

    def held(self):
        return self.down

    def pynput(self, name, log):
        from pynput import keyboard

        key = getattr(keyboard.Key, name, None) or keyboard.KeyCode.from_char(name)

        def press(pressed):
            if pressed == key:
                self.down = True

        def release(released):
            if released == key:
                self.down = False

        self.listener = keyboard.Listener(on_press=press, on_release=release)
        self.listener.start()
        self.listener.wait()
        if not getattr(self.listener, "IS_TRUSTED", True):
            raise PermissionError(
                "macOS hasn't allowed the terminal app to see the key: System Settings, Privacy & "
                "Security, Accessibility and Input Monitoring; then restart the terminal"
            )

    def evdev(self, name, log):
        """Straight from the input devices (a console, Wayland, buttons on the glasses as keys);
        needs read access to /dev/input (the input group). False if it can't be used."""
        try:
            import evdev
        except ImportError:
            return False
        code = evdev.ecodes.ecodes.get(name)
        devices = [evdev.InputDevice(path) for path in evdev.list_devices()]
        devices = [d for d in devices if code in d.capabilities().get(evdev.ecodes.EV_KEY, [])]
        if code is None or not devices:
            log(f"no input device with {name} can be read (is this user in the input group?)")
            return False

        def watch():
            import select

            while True:
                ready, _, _ = select.select(devices, [], [])
                for device in ready:
                    for event in device.read():
                        if event.type == evdev.ecodes.EV_KEY and event.code == code:
                            self.down = event.value != 0

        threading.Thread(target=watch, daemon=True).start()
        return True


class PushToTalk:
    """Listening only when asked: while the hold key is held (hold-to-talk), or after rynx's talk
    key (Space, Ctrl+Space or F2, or the menu's Talk) until a command is heard, with the words
    after "search" and the like, and "send", "again" or "cancel" after them. Nothing is recognised
    meanwhile, so nothing said in passing becomes a command."""

    # Seconds of listening after the talk key without hearing anything before giving up
    LIMIT = 8

    def __init__(self, rynx, key=None, clock=time.time):
        self.rynx = rynx
        self.key = key
        self.clock = clock
        self.on = False
        self.holding = False
        self.since = 0
        self.typing = False  # in a field when listening started: what's said is typed

    def listening(self):
        """off, started, on, or released (the hold key let go), asked for each piece of audio"""
        status = self.rynx.send("status")
        on = bool(status.get("listening"))
        held = self.key is not None and self.key.held()
        if held and not self.holding:
            self.holding = True
            if not on:
                self.rynx.send("talk on")  # TALK on rynx's top row
            on = True
        elif self.holding and not held:
            self.holding = self.on = False
            return "released"
        if on and not self.on:
            self.on, self.since = True, self.clock()
            self.typing = status.get("mode") in TYPING_MODES
            return "started"
        self.on = on
        if on and not self.holding and self.clock() - self.since > self.LIMIT:
            self.done("nothing heard (press Space to talk)")
            return "off"
        return "on" if on else "off"

    def heard(self, controller):
        """Something was heard: listening goes on while the key is held, or while words or send,
        again or cancel are to come; otherwise it stops"""
        if self.holding:
            return
        if controller.dictating or controller.typed is not None:
            self.since = self.clock()
        else:
            self.done()

    def done(self, message=None):
        self.rynx.send("talk off")
        if message:
            self.rynx.send(f"heard {message}")
        self.on = False


def recognise(chunks, controller, model_path, talk=None):
    """Feed audio chunks (16 kHz mono 16-bit) to the recognisers. Listening all the time, the
    grammar one, or after "search" and the like the free one. With push-to-talk or hold-to-talk
    (talk), only while asked, and both at once, so a command's words can follow it in one breath."""
    commands, free = recognisers(model_path)
    pending = []  # what the fixed recogniser heard of a phrase the free one hasn't finished

    def result(recogniser, finished):
        return json.loads(recogniser.Result() if finished else recogniser.FinalResult()).get("text", "")

    for chunk in chunks:
        if talk is None:
            recogniser = free if controller.dictating else commands
            if recogniser.AcceptWaveform(chunk):
                if not controller.heard(result(recogniser, True)):
                    return
            continue
        state = talk.listening()
        if state == "off":
            # Nothing half heard is kept for when listening starts (words awaited after "go to"
            # still are: they may come with the next press)
            commands.Reset()
            free.Reset()
            pending = []
            continue
        if state == "started" and talk.typing and not controller.dictating and controller.typed is None:
            controller.dictating = "type"
        if state == "released":
            said = controller.utterance(" ".join(pending + [result(commands, False)]), result(free, False))
            pending = []
            talk.done()
            if not said:
                return
            continue
        # The free recogniser says where a phrase ends: the fixed one, held to its phrases, ends
        # sooner and guesses at the rest ("search chocolate cake" was "search", then "eight")
        if commands.AcceptWaveform(chunk):
            pending.append(result(commands, True))
        if free.AcceptWaveform(chunk):
            heard = " ".join(pending + [result(commands, False)]), result(free, True)
            pending = []
            if not controller.utterance(*heard):
                return
            if any(t.replace("[unk]", "").strip() for t in heard):
                talk.heard(controller)
    if talk is None:
        for recogniser in (commands, free):
            text = result(recogniser, False)
            if text and not controller.heard(text):
                return
    else:
        controller.utterance(" ".join(pending + [result(commands, False)]), result(free, False))


def microphone():
    import sounddevice

    audio = queue.Queue()

    def callback(data, frames, timing, status):
        audio.put(bytes(data))

    with sounddevice.RawInputStream(samplerate=SAMPLE_RATE, blocksize=4000, dtype="int16", channels=1, callback=callback):
        while True:
            yield audio.get()


def wav_file(path):
    import wave

    with wave.open(str(path)) as w:
        if w.getframerate() != SAMPLE_RATE or w.getnchannels() != 1 or w.getsampwidth() != 2:
            sys.exit("The recording must be 16 kHz, mono, 16-bit")
        while chunk := w.readframes(4000):
            yield chunk
    # A moment of silence, so the last words are taken as finished
    yield b"\0" * SAMPLE_RATE


def main():
    cache = Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache") / "rynx"
    parser = argparse.ArgumentParser(description="Voice control for rynx (rynx --glasses)")
    parser.add_argument("--socket", type=Path, default=default_socket(), help="rynx's control socket")
    parser.add_argument("--model", type=Path, default=cache / MODEL_NAME, help="Vosk model folder")
    parser.add_argument("--download", action="store_true", help="download the speech model if it's missing")
    parser.add_argument("--check-model", action="store_true", help="only make sure the model is there")
    parser.add_argument("--text", action="store_true", help="type phrases instead of speaking")
    parser.add_argument("--wav", type=Path, help="recognise a recording instead of the microphone")
    parser.add_argument("--no-speech", action="store_true", help="don't speak answers aloud")
    parser.add_argument(
        "--push-to-talk", action="store_true", help="listen only after the talk key (Space in rynx), for one command"
    )
    parser.add_argument(
        "--hold-key",
        default="right_option" if sys.platform == "darwin" else "right_alt",
        help="with --push-to-talk, a key held to talk, read from the keyboard (right_option, right_ctrl, "
        "f13, KEY_... on Linux); none for only rynx's talk key",
    )
    parser.add_argument("--log", type=Path, help="write what's heard here instead of to the terminal")
    args = parser.parse_args()

    if args.check_model:
        ensure_model(args.model, args.download)
        return
    log_file = open(args.log, "a", buffering=1) if args.log else None

    def log(message):
        print(time.strftime("%H:%M:%S"), message, file=log_file or sys.stderr, flush=True)

    rynx = Rynx(args.socket, log)
    speaker = Speaker(not args.no_speech)
    controller = Controller(rynx, speaker, log)
    if not rynx.wait():
        sys.exit(f"rynx isn't answering at {args.socket}: start it with --glasses")
    log(f"connected to rynx at {args.socket}")

    if args.text:
        print('Type what you\'d say ("links", "follow three", "search", "help"); Ctrl+D ends.', file=sys.stderr)
        for line in sys.stdin:
            if not controller.heard(line):
                break
        return
    model = ensure_model(args.model, args.download)
    chunks = wav_file(args.wav) if args.wav else microphone()
    talk = None
    if args.push_to_talk:
        key = None
        if args.hold_key != "none" and not args.wav:
            try:
                key = HoldKey(args.hold_key, log)
            except Exception as e:
                log(f"hold-to-talk isn't available: {e}")
                rynx.send(f"heard hold-to-talk isn't available ({e}); Space works")
        talk = PushToTalk(rynx, key)
        held = args.hold_key.replace("_", " ")
        how = f"hold {held} while you speak, or press Space" if key else "press Space"
        rynx.send(f"heard voice control ready: {how}, then say a command (\"menu\", \"links\")")
        log(f"listening when asked ({how})")
    else:
        rynx.send("heard voice control ready: say \"links\", then a number")
        log("listening")
    try:
        recognise(chunks, controller, model, talk)
    except KeyboardInterrupt:
        pass
    except Exception as e:
        # Running in the background (scripts/rynx-glasses): rynx's status line is where it's seen
        log(f"stopped: {e!r}")
        rynx.send(f"heard voice control stopped: {e} (see the log)")
        raise
    finally:
        speaker.stop()


if __name__ == "__main__":
    main()

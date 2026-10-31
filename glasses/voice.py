#!/usr/bin/env python3
"""Voice control for rynx: say "links", then "follow twelve", and rynx follows link 12.

Speech is recognised on this computer with Vosk (nothing is sent anywhere), listening for a fixed
list of commands, which a small model recognises quickly even on a Raspberry Pi. After "search",
"go to", "find" or "type", the next thing said is taken as free words. Commands go to rynx's
control socket (rynx --glasses, or --control-socket), as action names, so keys.conf doesn't
matter.

    python3 glasses/voice.py             listen to the microphone
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
    # Handled here rather than by rynx
    "search": "@dictate open",
    "go to": "@dictate open",
    "find": "@dictate find",
    "type": "@dictate type",
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


class Controller:
    """Turns what was heard into commands, keeping track of whether free words are expected next"""

    def __init__(self, rynx, speaker, log):
        self.rynx = rynx
        self.speaker = speaker
        self.log = log
        self.dictating = None  # the command the next free words go to: "open", "find" or "type"

    def heard(self, text):
        """Something was heard. Returns False once rynx has quit."""
        text = text.strip()
        if not text or text == "[unk]":
            return True
        if self.dictating:
            command, self.dictating = self.dictating, None
            words = address_words(text) if command == "open" else text
            self.log(f"{command}: {words}")
            self.rynx.send(f"heard {command} {words}")
            return self.run(f"{command} {words}")
        # Several commands without a pause between them: each in turn
        parts = split_commands(text) if understand(text) is None else None
        if parts and len(parts) > 1:
            return all(self.heard(part) for part in parts)
        # While speaking, only "stop" is listened for: the microphone hears the voice too
        meaning = understand(text)
        if self.speaker.speaking() and meaning != "@stop":
            return True
        # A number on its own follows a link only while the links are numbered (after "links"):
        # otherwise a stray "to", heard as two, would follow one
        if meaning and meaning.startswith("follow") and number(text.replace("[unk]", "").split()) is not None:
            if self.rynx.send("status").get("mode") != "Hints":
                meaning = None
        if meaning is None:
            self.log(f"not a command: {text}")
            self.rynx.send(f"heard {text} (not a command; say help)")
            return True
        self.log(f"{text} -> {meaning}")
        self.rynx.send(f"heard {text}")
        if meaning.startswith("@dictate "):
            self.dictating = meaning.split()[1]
            prompt = {"open": "Where to?", "find": "Find what?", "type": "Type what?"}[self.dictating]
            self.rynx.send(f"heard {text}: {prompt} (say it now)")
            self.speaker.say(prompt)
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
            self.speaker.say("Say links, then a number. Or down, up, back, search, go to, find, read, new tab, next tab.")
            return True
        return self.run(meaning)

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


def recognise(chunks, controller, model_path):
    """Feed audio chunks (16 kHz mono 16-bit) to the recogniser, the grammar one or, after
    "search" and the like, the free one"""
    commands, free = recognisers(model_path)
    for chunk in chunks:
        recogniser = free if controller.dictating else commands
        if recogniser.AcceptWaveform(chunk):
            text = json.loads(recogniser.Result()).get("text", "")
            if not controller.heard(text):
                return
    for recogniser in (commands, free):
        text = json.loads(recogniser.FinalResult()).get("text", "")
        if text and not controller.heard(text):
            return


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
    rynx.send("heard voice control ready: say \"links\", then a number")
    log("listening")
    try:
        recognise(chunks, controller, model)
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

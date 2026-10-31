#!/usr/bin/env python3
"""Tests for voice.py: python3 glasses/test_voice.py

The phrase tests need nothing installed. The speech test needs Vosk, its model (VOSK_MODEL=path)
and macOS's say, which records the phrases spoken; it's skipped without them.
"""

import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import unittest
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import voice  # noqa: E402


class Phrases(unittest.TestCase):
    def test_commands_and_numbers(self):
        self.assertEqual(voice.understand("links"), "hints")
        self.assertEqual(voice.understand("Follow twelve"), "follow 12")
        self.assertEqual(voice.understand("twenty one"), "follow 21")
        self.assertEqual(voice.understand("follow to"), "follow 2", "heard as 'to'")
        self.assertEqual(voice.understand("open three in new tab"), "follow_in_new_tab 3")
        self.assertEqual(voice.understand("go to"), "@dictate open")
        self.assertIsNone(voice.understand("follow zero"))
        self.assertIsNone(voice.understand("follow banana"))
        self.assertIsNone(voice.understand("delete everything"))

    def test_every_phrase_is_in_the_grammar(self):
        grammar = voice.grammar()
        self.assertIn("follow forty two", grammar)
        self.assertIn("go to", grammar)
        for said in grammar:
            self.assertIsNotNone(voice.understand(said), said)

    def test_commands_said_together(self):
        self.assertEqual(voice.split_commands("links follow three"), ["links", "follow three"])
        self.assertEqual(voice.split_commands("go back next tab"), ["go back", "next tab"])
        self.assertEqual(voice.split_commands("open three in new tab"), ["open three in new tab"])
        self.assertIsNone(voice.split_commands("links banana"))

    def test_addresses_said_aloud(self):
        self.assertEqual(voice.address_words("wikipedia dot org"), "wikipedia.org")
        self.assertEqual(voice.address_words("example dot com slash news"), "example.com/news")
        self.assertEqual(voice.address_words("terminal browsers"), "terminal browsers")


class FakeRynx:
    """A control socket that records commands and answers ok"""

    def __init__(self, path):
        self.commands = []
        self.server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.server.bind(str(path))
        self.server.listen()
        threading.Thread(target=self.serve, daemon=True).start()

    def serve(self):
        while True:
            connection, _ = self.server.accept()
            stream = connection.makefile("rwb")
            for line in stream:
                self.commands.append(line.decode().strip())
                stream.write(json.dumps({"ok": True, "message": "", "title": "Test"}).encode() + b"\n")
                stream.flush()


class Controller(unittest.TestCase):
    def test_typed_phrases_become_commands(self):
        with tempfile.TemporaryDirectory(dir="/tmp") as folder:
            path = Path(folder) / "s.sock"
            rynx = FakeRynx(path)
            controller = voice.Controller(voice.Rynx(path, print), voice.Speaker(False), lambda m: None)
            for said in ["links", "follow four", "go to", "wikipedia dot org", "banana"]:
                controller.heard(said)
            sent = [c for c in rynx.commands if not c.startswith("heard")]
            self.assertEqual(sent, ["hints", "follow 4", "open wikipedia.org"])
            self.assertIn("heard banana (not a command; say help)", rynx.commands)


@unittest.skipUnless(
    shutil.which("say") and shutil.which("afconvert") and os.environ.get("VOSK_MODEL"),
    "needs macOS's say and a Vosk model (VOSK_MODEL=path)",
)
class Speech(unittest.TestCase):
    def test_spoken_commands_are_recognised(self):
        with tempfile.TemporaryDirectory(dir="/tmp") as folder:
            folder = Path(folder)
            path = folder / "s.sock"
            rynx = FakeRynx(path)
            # Each phrase recorded with a pause after it, as one 16 kHz mono recording
            phrases = ["links", "follow seven", "scroll down", "go back", "next tab"]
            text = " [[slnc 900]] ".join(phrases) + " [[slnc 900]]"
            subprocess.run(["say", "-o", str(folder / "s.aiff"), text], check=True)
            subprocess.run(
                ["afconvert", "-f", "WAVE", "-d", "LEI16@16000", "-c", "1", str(folder / "s.aiff"), str(folder / "s.wav")],
                check=True,
            )
            controller = voice.Controller(voice.Rynx(path, print), voice.Speaker(False), lambda m: None)
            voice.recognise(voice.wav_file(folder / "s.wav"), controller, Path(os.environ["VOSK_MODEL"]))
            sent = [c for c in rynx.commands if not c.startswith("heard")]
            self.assertEqual(sent, ["hints", "follow 7", "half_page_down", "back", "next_tab"])


if __name__ == "__main__":
    unittest.main()

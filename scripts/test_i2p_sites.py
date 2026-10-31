#!/usr/bin/env python3
"""Tests for i2p_sites.py that need no network: python3 scripts/test_i2p_sites.py"""

import base64
import hashlib
import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import i2p_sites  # noqa: E402


def destination(seed):
    """A destination-shaped string (391 bytes, in I2P's base 64), and its .b32.i2p address"""
    raw = hashlib.sha256(seed).digest() * 12 + b"\x05\x00\x04\x00\x07\x00\x00"
    text = base64.b64encode(raw).decode().replace("+", "-").replace("/", "~")
    return text, base64.b32encode(hashlib.sha256(raw).digest()).decode().rstrip("=").lower() + ".b32.i2p"


class AddressBooks(unittest.TestCase):
    def test_names_and_addresses_are_read(self):
        one, one_b32 = destination(b"one")
        two, two_b32 = destination(b"two")
        book = "\n".join([
            "# a comment",
            f"one.i2p={one}#!sig=abc~def",
            f"Two.I2P={two}",
            f"not a name.i2p={one}",
            f"clearweb.com={one}",
            "short.i2p=AAAA",
            "",
        ])
        self.assertEqual(i2p_sites.parse_address_book(book), {"one.i2p": one_b32, "two.i2p": two_b32})

    def test_a_real_registry_address(self):
        # What the registries are reached by must be the address of their own entry: the script's
        # REGISTRIES were checked against reg.i2p's, notbob.i2p's and stats.i2p's destinations
        for url in i2p_sites.REGISTRIES.values():
            host = url.split("/")[2]
            self.assertRegex(host, r"^[a-z2-7]{52}\.b32\.i2p$")


class Pages(unittest.TestCase):
    def test_title_description_and_heading(self):
        info = i2p_sites.PageInfo()
        info.feed("<html><head><title> A  site </title><meta name=description content='About it'></head>"
                  "<body><h1>Welcome</h1><h1>Second</h1></body></html>")
        self.assertEqual(i2p_sites.tidy(info.title), "A site")
        self.assertEqual(info.description, "About it")
        self.assertEqual(i2p_sites.tidy(info.heading), "Welcome")

    def test_control_characters_never_reach_the_terminal(self):
        self.assertEqual(i2p_sites.tidy("Title\x1b]0;owned\x07 here\x1b[2J"), "Title]0;owned here[2J")
        self.assertEqual(i2p_sites.tidy("x" * 300, 10), "xxxxxxxxx…")


if __name__ == "__main__":
    unittest.main()

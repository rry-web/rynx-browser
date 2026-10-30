#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
#
# rynx-reticulum-bridge: lets rynx browse NomadNet pages on the Reticulum network.
#
# rynx asks for http://127.0.0.1:4243/<node>/<path>?<variables>, and the bridge fetches the page
# from the NomadNet node over Reticulum, as NomadNet's own browser does: it finds a path to the
# node, opens an encrypted link to its nomadnetwork.node destination, and requests the path, with
# the variables and fields as request data ("var_..." and "field_..."). Pages come back as Micron
# (text/micron), files as downloads.
#
# It uses Reticulum's reference software (pip install rns): if rnsd is running, through its
# shared instance; otherwise it starts Reticulum itself with your configuration (~/.reticulum).
# It listens on 127.0.0.1 only, and answers only requests carrying rynx's X-Rynx header, so web
# pages in other browsers can't use it.
#
# Usage: rynx_reticulum_bridge.py [--port 4243] [--config DIR] [--timeout SECONDS]
#
# This file is under the MIT licence, so that it sits well beside Reticulum's own licence
# (https://github.com/markqvist/Reticulum/blob/master/LICENSE), which it uses but doesn't include.

import argparse
import http.server
import re
import sys
import threading
import time
import urllib.parse

try:
    import RNS
except ImportError:
    sys.exit("Reticulum isn't installed: pip install rns")

APP_NAME = "nomadnetwork"
ASPECT = "node"
DEFAULT_PATH = "/page/index.mu"
NODE = re.compile(r"^[0-9a-f]{32}$")
# A page larger than this isn't a page
MAX_RESPONSE = 32 * 1024 * 1024


class Bridge:
    """Links to NomadNet nodes, opened as needed and kept for the next request"""

    def __init__(self, timeout):
        self.timeout = timeout
        self.links = {}
        self.lock = threading.Lock()
        self.node_locks = {}

    def node_lock(self, node):
        with self.lock:
            return self.node_locks.setdefault(node, threading.Lock())

    def link_to(self, node, deadline):
        """An active link to the node, opening one if needed; raises BridgeError"""
        link = self.links.get(node)
        if link is not None and link.status == RNS.Link.ACTIVE:
            return link
        if not RNS.Transport.has_path(node):
            RNS.Transport.request_path(node)
            while not RNS.Transport.has_path(node):
                if time.time() > deadline:
                    raise BridgeError(504, "No path to the node",
                        "Reticulum found no way to reach it. It may be offline, out of reach of "
                        "your interfaces, or not announced yet; try again in a while.")
                time.sleep(0.25)
        # The link is anonymous: the node learns nothing about who asks
        identity = RNS.Identity.recall(node)
        if identity is None:
            raise BridgeError(502, "The node is unknown", "Its identity hasn't been announced yet.")
        destination = RNS.Destination(identity, RNS.Destination.OUT, RNS.Destination.SINGLE, APP_NAME, ASPECT)
        established = threading.Event()
        link = RNS.Link(destination, established_callback=lambda l: established.set(),
                        closed_callback=lambda l: established.set())
        while not established.wait(0.25):
            if time.time() > deadline:
                link.teardown()
                raise BridgeError(504, "The node didn't answer", "No link could be opened in time.")
        if link.status != RNS.Link.ACTIVE:
            raise BridgeError(502, "The node refused the link", "It closed the link as it opened.")
        self.links[node] = link
        return link

    def fetch(self, node, path, data):
        """The node's response to a request for path: (bytes, filename or None)"""
        deadline = time.time() + self.timeout
        with self.node_lock(node):
            link = self.link_to(node, deadline)
        done = threading.Event()
        result = {}

        def received(receipt):
            result["receipt"] = receipt
            done.set()

        def failed(receipt):
            done.set()

        receipt = link.request(path, data=data, response_callback=received, failed_callback=failed,
                               timeout=max(1, deadline - time.time()))
        if not receipt:
            self.links.pop(node, None)
            raise BridgeError(502, "The request couldn't be sent", "The link to the node closed.")
        if not done.wait(max(1, deadline - time.time()) + 5) or "receipt" not in result:
            raise BridgeError(504, "The node didn't send the page", "The request timed out or failed.")
        return response_body(result["receipt"])


def response_body(receipt):
    """A response as bytes, with a file's name when it's a file"""
    response = receipt.response
    if isinstance(response, bytes):
        return response, None
    if isinstance(response, str):
        return response.encode("utf-8"), None
    # A file sent as a resource: an open file, with its name in the metadata
    if hasattr(response, "read"):
        name = (receipt.metadata or {}).get("name") if isinstance(receipt.metadata, dict) else None
        data = response.read(MAX_RESPONSE + 1)
        return data, safe_name(name or "download")
    # A file as [name, data]
    if isinstance(response, (list, tuple)) and len(response) == 2:
        name, data = response
        if isinstance(name, bytes):
            name = name.decode("utf-8", "replace")
        if isinstance(data, str):
            data = data.encode("utf-8")
        return bytes(data), safe_name(str(name))
    raise BridgeError(502, "The node sent nothing readable", "Its answer wasn't a page or a file.")


def safe_name(name):
    name = name.replace("\\", "/").split("/")[-1]
    name = re.sub(r"[^\w.\- ]", "_", name).strip(" .")
    return name or "download"


def request_data(query):
    """Request data as NomadNet sends it: variables as var_<name>, fields as field_<name>"""
    data = {}
    for key, value in urllib.parse.parse_qsl(query, keep_blank_values=True):
        if not (key.startswith("var_") or key.startswith("field_")):
            key = "var_" + key
        data[key] = value
    return data or None


class BridgeError(Exception):
    def __init__(self, status, title, detail):
        super().__init__(title)
        self.status, self.title, self.detail = status, title, detail


def micron_escape(text):
    return text.replace("\\", "\\\\").replace("`", "\\`")


class Handler(http.server.BaseHTTPRequestHandler):
    bridge = None
    port = None
    server_version = "rynx-reticulum-bridge"

    def do_GET(self):
        # Only rynx: other browsers' pages can't add this header without asking first, and a
        # Host of this computer turns away pages using DNS rebinding
        host = (self.headers.get("Host") or "").split(":")[0]
        if self.headers.get("X-Rynx") != "1" or host not in ("127.0.0.1", "localhost"):
            return self.reply(403, b"Only rynx may use this bridge\n", "text/plain")
        target = urllib.parse.urlsplit(self.path)
        if target.path in ("", "/"):
            return self.status_page()
        _, node, path = (target.path.split("/", 2) + [""])[:3]
        node = node.lower()
        if not NODE.match(node):
            return self.error(BridgeError(400, "Not a node's address",
                "A NomadNet address starts with the node's 32 hexadecimal digits."))
        path = "/" + urllib.parse.unquote(path) if path else DEFAULT_PATH
        try:
            body, filename = self.bridge.fetch(bytes.fromhex(node), path, request_data(target.query))
        except BridgeError as e:
            return self.error(e)
        if len(body) > MAX_RESPONSE:
            return self.error(BridgeError(502, "Too large", "The node's answer is larger than 32 MB."))
        if filename:
            return self.reply(200, body, "application/octet-stream",
                              {"Content-Disposition": 'attachment; filename="%s"' % filename})
        self.reply(200, body, "text/micron; charset=utf-8")

    def status_page(self):
        interfaces = list(RNS.Transport.interfaces)
        lines = [">rynx Reticulum bridge", "", "Running, on port %d." % self.port,
                 "Reticulum interfaces: %d" % len(interfaces)]
        for interface in interfaces:
            lines.append("  " + micron_escape(str(interface)))
        self.reply(200, ("\n".join(lines) + "\n").encode("utf-8"), "text/micron; charset=utf-8")

    def error(self, e):
        page = ">%s\n\n%s\n" % (micron_escape(e.title), micron_escape(e.detail))
        self.reply(e.status, page.encode("utf-8"), "text/micron; charset=utf-8")

    def reply(self, status, body, content_type, headers=None):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        for name, value in (headers or {}).items():
            self.send_header(name, value)
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format, *args):
        RNS.log("Bridge: " + format % args, RNS.LOG_DEBUG)


def main():
    parser = argparse.ArgumentParser(description="Lets rynx browse NomadNet pages on the Reticulum network")
    parser.add_argument("--port", type=int, default=4243, help="port on 127.0.0.1 to serve rynx (4243)")
    parser.add_argument("--config", help="Reticulum configuration folder (~/.reticulum)")
    parser.add_argument("--timeout", type=float, default=60, help="seconds to wait for a page (60)")
    parser.add_argument("--verbose", action="store_true", help="log Reticulum's messages")
    args = parser.parse_args()

    RNS.Reticulum(configdir=args.config, loglevel=RNS.LOG_INFO if args.verbose else RNS.LOG_WARNING)
    Handler.bridge = Bridge(args.timeout)
    Handler.port = args.port
    server = http.server.ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    server.daemon_threads = True
    print("rynx-reticulum-bridge: serving rynx at http://127.0.0.1:%d" % args.port, flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()

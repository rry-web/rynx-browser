#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
#
# The bridge against a NomadNet-style node, over a private Reticulum instance with no interfaces:
# nothing leaves this computer. Needs Reticulum (pip install rns).
#
# Usage: test_bridge.py   (exits 0 when every check passes)

import os
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
BRIDGE = os.path.join(HERE, "rynx_reticulum_bridge.py")

CONFIG = """[reticulum]
  enable_transport = False
  share_instance = Yes
  shared_instance_port = %d
  instance_control_port = %d
  panic_on_interface_error = No

[logging]
  loglevel = 2

[interfaces]
"""

NODE = r'''
import sys, time, RNS
RNS.Reticulum(configdir=sys.argv[1], loglevel=RNS.LOG_WARNING)
destination = RNS.Destination(RNS.Identity(), RNS.Destination.IN, RNS.Destination.SINGLE, "nomadnetwork", "node")
def index(path, data, request_id, link_id, remote_identity, requested_at):
    return b">Test node\n`!Welcome`! to the test node\n`[About`:/page/about.mu]\n"
def echo(path, data, request_id, link_id, remote_identity, requested_at):
    pairs = sorted((data or {}).items())
    return ("\n".join("%s=%s" % kv for kv in pairs) + "\n").encode()
def anonymous(path, data, request_id, link_id, remote_identity, requested_at):
    return b"identified\n" if remote_identity else b"anonymous\n"
def file(path, data, request_id, link_id, remote_identity, requested_at):
    return ["hello.txt", b"Hello from Reticulum\n"]
for path, handler in [("/page/index.mu", index), ("/page/echo.mu", echo), ("/page/who.mu", anonymous), ("/file/hello.txt", file)]:
    destination.register_request_handler(path, response_generator=handler, allow=RNS.Destination.ALLOW_ALL)
destination.announce()
print(RNS.hexrep(destination.hash, delimit=False), flush=True)
while True:
    time.sleep(1)
'''


def get(port, path, headers=None):
    request = urllib.request.Request("http://127.0.0.1:%d%s" % (port, path), headers={"X-Rynx": "1"} if headers is None else headers)
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            return response.status, dict(response.headers), response.read().decode()
    except urllib.error.HTTPError as e:
        return e.code, dict(e.headers), e.read().decode()


def main():
    failures = []

    def check(what, ok, detail=""):
        print(("ok    " if ok else "FAIL  ") + what + ("" if ok else ": " + str(detail)))
        if not ok:
            failures.append(what)

    with tempfile.TemporaryDirectory() as config:
        base = 40000 + os.getpid() % 10000
        with open(os.path.join(config, "config"), "w") as f:
            f.write(CONFIG % (base, base + 1))
        port = base + 2
        node = subprocess.Popen([sys.executable, "-I", "-c", NODE, config], stdout=subprocess.PIPE, text=True)
        bridge = None
        try:
            node_hash = node.stdout.readline().strip()
            check("the node started", len(node_hash) == 32, node_hash)
            bridge = subprocess.Popen([sys.executable, "-I", BRIDGE, "--config", config, "--port", str(port), "--timeout", "20"],
                                      stdout=subprocess.PIPE, text=True)
            ready = bridge.stdout.readline()
            check("the bridge started", "serving rynx" in ready, ready)

            status, headers, body = get(port, "/%s/page/index.mu" % node_hash)
            check("a page", status == 200 and "Welcome" in body, (status, body))
            check("as Micron", headers.get("Content-Type", "").startswith("text/micron"), headers)

            status, _, body = get(port, "/%s/" % node_hash)
            check("the node's index by default", status == 200 and "Test node" in body, (status, body))

            status, _, body = get(port, "/%s/page/echo.mu?action=view&n=64&field_name=Ray&var_x=1" % node_hash)
            check("variables and fields as NomadNet sends them",
                  body.split() == ["field_name=Ray", "var_action=view", "var_n=64", "var_x=1"], body)

            status, _, body = get(port, "/%s/page/who.mu" % node_hash)
            check("anonymously", body.strip() == "anonymous", body)

            status, headers, body = get(port, "/%s/file/hello.txt" % node_hash)
            check("a file, as a download", status == 200 and body == "Hello from Reticulum\n"
                  and 'filename="hello.txt"' in headers.get("Content-Disposition", ""), (status, headers, body))

            status, _, body = get(port, "/%s/page/missing.mu" % node_hash)
            check("a page the node doesn't have", status >= 500, (status, body))

            status, _, body = get(port, "/%s/page/index.mu" % ("0" * 32))
            check("a node that can't be reached", status == 504 and "No path" in body, (status, body))

            status, _, body = get(port, "/not-a-node/page/index.mu")
            check("an address that isn't a node's", status == 400, (status, body))

            status, _, _ = get(port, "/%s/page/index.mu" % node_hash, headers={})
            check("not without rynx's header", status == 403, status)
            status, _, _ = get(port, "/%s/page/index.mu" % node_hash, headers={"X-Rynx": "1", "Host": "evil.example"})
            check("not by another name (DNS rebinding)", status == 403, status)

            status, _, body = get(port, "/")
            check("a status page", status == 200 and "Running" in body, (status, body))
        finally:
            for process in (bridge, node):
                if process:
                    process.terminate()
                    process.wait(10)

    print("%d failed" % len(failures) if failures else "all passed")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Find I2P sites worth a look: gather the registries' address books, see which sites answer, and
search their names, titles and descriptions.

    scripts/i2p_sites.py update               # fetch the registries' address books
    scripts/i2p_sites.py check --limit 200    # visit sites not seen lately (front pages only)
    scripts/i2p_sites.py check --down         # and try again those that were down
    scripts/i2p_sites.py search music radio   # sites matching any of the words, best first
    scripts/i2p_sites.py new                  # sites the registries added most recently
    scripts/i2p_sites.py stats

Everything goes through the I2P router's HTTP proxy (--proxy, 127.0.0.1:4444 by default), with the
generic identity rynx uses in I2P mode. Sites are reached by their .b32.i2p address, worked out from
the destination in the address book, so the router's own address book is neither needed nor
changed. Checking is gentle on a slow network: a few sites at a time, only each front page, at
most 256 KB of it, nothing it links to, and no redirects followed. What's found is kept in a JSON
file (--db), saved as it goes, so an interrupted check loses nothing.

Registries list whatever was registered: titles can be anything, including what you'd rather not
see. Nothing but text is fetched.
"""

import argparse
import base64
import concurrent.futures
import hashlib
import html.parser
import json
import os
import re
import sys
import threading
import time
import urllib.error
import urllib.request

# The registries' address books, by their .b32.i2p addresses (the names may not be in the router's
# address book yet)
REGISTRIES = {
    "reg.i2p": "http://shx5vqsw7usdaunyzr2qmes2fq37oumybpudrd4jjj4e4vk4uusa.b32.i2p/export/hosts.txt",
    "notbob.i2p": "http://nytzrhrjjfsutowojvxi7hphesskpqqr65wpistz6wa7cpajhp7a.b32.i2p/hosts.txt",
    "stats.i2p": "http://kqypgjpjwrphnzebod5ev3ts2vtii6e5tntrg4rnfijqc7rypldq.b32.i2p/cgi-bin/newhosts.txt",
}

# What rynx sends in I2P mode, and what the router's proxy rewrites identities to anyway
USER_AGENT = "MYOB/6.66 (AN/ON)"
TIMEOUT = 90
MAX_PAGE = 256 * 1024
MAX_ADDRESS_BOOK = 16 * 1024 * 1024
DAY = 24 * 60 * 60
NAME = re.compile(r"^[a-z0-9]([a-z0-9.-]{0,250}[a-z0-9])?\.i2p$")


def default_db():
    cache = os.environ.get("XDG_CACHE_HOME") or os.path.join(os.path.expanduser("~"), ".cache")
    return os.path.join(cache, "rynx", "i2p-sites.json")


def b32_address(destination):
    """A destination's .b32.i2p address: the SHA-256 of its bytes, in base 32. None if it isn't
    one (I2P's base 64 uses - and ~ for + and /)"""
    try:
        raw = base64.b64decode(destination.replace("-", "+").replace("~", "/"), validate=True)
    except ValueError:
        return None
    # A destination is at least two 256-byte keys and a certificate
    if len(raw) < 387:
        return None
    return base64.b32encode(hashlib.sha256(raw).digest()).decode().rstrip("=").lower() + ".b32.i2p"


def parse_address_book(text):
    """{name: b32 address} from an address book's "name=destination#!extensions" lines"""
    sites = {}
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        name, destination = line.split("=", 1)
        name = name.strip().lower()
        destination = destination.split("#", 1)[0].strip()
        address = b32_address(destination)
        if NAME.match(name) and address:
            sites[name] = address
    return sites


class PageInfo(html.parser.HTMLParser):
    """A page's title, description and first heading"""

    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.title = ""
        self.description = ""
        self.heading = ""
        self._in = None
        self._done = set()

    def handle_starttag(self, tag, attrs):
        attrs = {k.lower(): (v or "") for k, v in attrs}
        if tag == "meta" and not self.description:
            key = (attrs.get("name") or attrs.get("property") or "").lower()
            if key in ("description", "og:description"):
                self.description = attrs.get("content", "")
        # Only the first title and heading
        elif tag in ("title", "h1") and self._in is None and tag not in self._done:
            self._in = tag

    def handle_endtag(self, tag):
        if tag == self._in:
            self._done.add(tag)
            self._in = None

    def handle_data(self, data):
        if self._in == "title" and len(self.title) < 300:
            self.title += data
        elif self._in == "h1" and len(self.heading) < 300:
            self.heading += data


def tidy(text, limit=200):
    text = re.sub(r"\s+", " ", text or "").strip()
    # Control characters could act on the terminal the results are printed to
    text = "".join(c for c in text if c.isprintable())
    return text[: limit - 1] + "…" if len(text) > limit else text


class NoRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


def opener(proxy):
    return urllib.request.build_opener(urllib.request.ProxyHandler({"http": proxy, "https": proxy}), NoRedirects())


def get(open_url, url, limit):
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT, "Accept": "text/html,*/*;q=0.5"})
    try:
        with open_url.open(request, timeout=TIMEOUT) as response:
            return response.status, response.headers, response.read(limit)
    except urllib.error.HTTPError as e:
        return e.code, e.headers, e.read(limit) if e.fp else b""


class Database:
    def __init__(self, path):
        self.path = path
        self.lock = threading.Lock()
        try:
            with open(path, encoding="utf-8") as f:
                self.sites = json.load(f)
        except FileNotFoundError:
            self.sites = {}

    def save(self):
        with self.lock:
            os.makedirs(os.path.dirname(self.path) or ".", exist_ok=True)
            temporary = self.path + ".tmp"
            with open(temporary, "w", encoding="utf-8") as f:
                json.dump(self.sites, f, indent=1, sort_keys=True)
            os.replace(temporary, self.path)


def update(db, proxy):
    open_url = opener(proxy)
    now = int(time.time())
    for registry, url in REGISTRIES.items():
        print(f"{registry}: fetching its address book...", flush=True)
        # I2P connections drop now and then; a second try usually gets through
        for attempt in (1, 2, 3):
            try:
                status, _, body = get(open_url, url, MAX_ADDRESS_BOOK)
                break
            except (OSError, urllib.error.URLError) as e:
                print(f"  couldn't: {e}" + (", trying again" if attempt < 3 else ""), flush=True)
        else:
            continue
        if status != 200:
            print(f"  it answered {status}")
            continue
        found = parse_address_book(body.decode("utf-8", "replace"))
        added = 0
        for name, address in found.items():
            site = db.sites.setdefault(name, {"first_seen": now})
            added += "address" not in site
            if site.get("address") not in (None, address):
                # A name re-registered to another destination: what was found before is stale
                site.pop("checked", None)
            site["address"] = address
            site["registries"] = sorted(set(site.get("registries", [])) | {registry})
        print(f"  {len(found)} names, {added} new")
    db.save()
    print(f"{len(db.sites)} sites known; `check` visits them")


def check_site(open_url, name, site):
    url = f"http://{site['address']}/"
    result = {"checked": int(time.time())}
    try:
        status, headers, body = get(open_url, url, MAX_PAGE)
    except Exception as e:  # noqa: BLE001 - any failure means the site didn't answer
        result.update(up=False, error=tidy(str(e), 120))
        return result
    # The router's own pages (500, 503, 504) mean the site couldn't be reached
    result["status"] = status
    result["up"] = status < 500
    if 300 <= status < 400:
        result["redirect"] = tidy(headers.get("Location", ""), 200)
    kind = headers.get("Content-Type", "")
    if result["up"] and "html" in kind.lower():
        info = PageInfo()
        try:
            info.feed(body.decode(headers.get_content_charset() or "utf-8", "replace"))
        except Exception:  # noqa: BLE001 - a broken page still answered
            pass
        result.update(title=tidy(info.title), description=tidy(info.description, 300), heading=tidy(info.heading))
    elif result["up"]:
        result["type"] = tidy(kind, 60)
    return result


def check(db, proxy, limit, parallel, days, down):
    now = int(time.time())
    due = [
        n
        for n, s in db.sites.items()
        if "address" in s and (now - s.get("checked", 0) > days * DAY or (down and not s.get("up")))
    ]
    # Never-checked first, then the longest ago
    due.sort(key=lambda n: db.sites[n].get("checked", 0))
    due = due[:limit]
    if not due:
        print("Every site has been checked lately (--days changes how lately)")
        return
    print(f"Checking {len(due)} sites, {parallel} at a time (Ctrl+C stops, keeping what's found)...", flush=True)
    open_url = opener(proxy)
    done = up = 0
    pool = concurrent.futures.ThreadPoolExecutor(max_workers=parallel)
    futures = {pool.submit(check_site, open_url, name, db.sites[name]): name for name in due}
    try:
        for future in concurrent.futures.as_completed(futures):
            name = futures[future]
            result = future.result()
            with db.lock:
                site = db.sites[name]
                for key in ("status", "error", "redirect", "type", "title", "description", "heading"):
                    site.pop(key, None)
                site.update(result)
                if result["up"]:
                    site["last_up"] = result["checked"]
            done += 1
            up += result["up"]
            mark = "up  " if result["up"] else "down"
            print(f"[{done}/{len(due)}] {mark} {name}  {result.get('title', '')}", flush=True)
            if done % 20 == 0:
                db.save()
    except KeyboardInterrupt:
        print("\nStopping; saving what was found")
        pool.shutdown(wait=False, cancel_futures=True)
    else:
        pool.shutdown()
    db.save()
    print(f"{up} of {done} answered")


def show(name, site):
    state = "up" if site.get("up") else ("down" if "checked" in site else "not checked")
    if site.get("last_up") and not site.get("up"):
        state += ", last up " + time.strftime("%Y-%m-%d", time.gmtime(site["last_up"]))
    print(f"{name}  ({state}; {', '.join(site.get('registries', []))})")
    for key in ("title", "heading", "description"):
        if site.get(key) and (key != "heading" or site[key] != site.get("title")):
            print(f"    {site[key]}")
    if site.get("redirect"):
        print(f"    redirects to {site['redirect']}")
    if site.get("type"):
        print(f"    serves {site['type']}")
    print(f"    http://{name}/   http://{site.get('address', '')}/")


def search(db, words, exclude, everything):
    words = [w.lower() for w in words]
    exclude = [w.lower() for w in exclude]
    found = []
    for name, site in db.sites.items():
        if not everything and not site.get("up"):
            continue
        text = " ".join([name, site.get("title", ""), site.get("heading", ""), site.get("description", "")]).lower()
        if any(w in text for w in exclude):
            continue
        # The more words matched the better; a match in the name counts double
        score = sum(text.count(w) + 2 * (w in name) for w in words) if words else 1
        if score:
            found.append((-score, name))
    for _, name in sorted(found):
        show(name, db.sites[name])
    print(f"{len(found)} sites" + ("" if everything else " that answered when checked (--all for every one)"))


def new(db, count):
    # Seen first most lately; among those seen at once (the first update), stats.i2p's, whose list
    # is of names registered lately
    def recent(name):
        site = db.sites[name]
        return (site.get("first_seen", 0), "stats.i2p" in site.get("registries", []))

    names = sorted(db.sites, key=recent, reverse=True)[:count]
    for name in names:
        show(name, db.sites[name])


def stats(db):
    checked = [s for s in db.sites.values() if "checked" in s]
    up = [s for s in checked if s.get("up")]
    print(f"{len(db.sites)} sites known, {len(checked)} checked, {len(up)} answered")
    for registry in REGISTRIES:
        print(f"  {registry}: {sum(registry in s.get('registries', []) for s in db.sites.values())}")
    print(f"Kept in {db.path}")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--proxy", default="http://127.0.0.1:4444", help="the I2P router's HTTP proxy")
    parser.add_argument("--db", default=default_db(), help="where what's found is kept")
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("update", help="fetch the registries' address books")
    c = commands.add_parser("check", help="visit sites' front pages")
    c.add_argument("--limit", type=int, default=100, help="sites to visit this time (100)")
    c.add_argument("--parallel", type=int, default=4, help="sites visited at once (4)")
    c.add_argument("--days", type=float, default=7, help="revisit sites checked longer ago than this (7)")
    c.add_argument("--down", action="store_true", help="also try again the sites that were down (I2P drops connections)")
    s = commands.add_parser("search", help="sites whose name, title or description has any of the words")
    s.add_argument("words", nargs="*")
    s.add_argument("--exclude", nargs="*", default=[], help="leave out sites with any of these words")
    s.add_argument("--all", action="store_true", help="include sites that didn't answer or weren't checked")
    n = commands.add_parser("new", help="the sites registered most recently")
    n.add_argument("--count", type=int, default=20)
    commands.add_parser("stats", help="how many sites are known, checked and up")
    args = parser.parse_args()

    db = Database(args.db)
    if args.command == "update":
        update(db, args.proxy)
    elif args.command == "check":
        check(db, args.proxy, max(1, args.limit), max(1, min(args.parallel, 16)), args.days, args.down)
    elif args.command == "search":
        search(db, args.words, args.exclude, args.all)
    elif args.command == "new":
        new(db, args.count)
    else:
        stats(db)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        sys.exit(130)

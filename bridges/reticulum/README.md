# rynx's Reticulum bridge

Lets rynx browse NomadNet pages on the [Reticulum](https://reticulum.network) network: the
encrypted mesh that runs over LoRa radio, packet radio, serial links, ordinary networks and I2P.

rynx shows NomadNet pages (Micron) itself; this small program fetches them over Reticulum, the way
NomadNet's own browser does, and hands them to rynx on this computer.

## Setting it up

1. Install Reticulum, Python 3.9 or newer:

   ```sh
   pip install rns
   ```

   If you don't use Reticulum yet, its first start writes a configuration (`~/.reticulum/config`)
   that finds other Reticulum devices on your local network. To reach the wider network, add an
   interface, for example a TCP connection to a public node: see the
   [Reticulum manual](https://reticulum.network/manual/interfaces.html).

2. Start the bridge, and leave it running:

   ```sh
   python3 rynx_reticulum_bridge.py
   ```

   If `rnsd` (Reticulum's background service) is running, the bridge uses it; otherwise it starts
   Reticulum itself. Options: `--port` (4243), `--config` (Reticulum's folder), `--timeout` (60
   seconds for a page; slow radio links may need more), `--verbose`.

3. Open a NomadNet address in rynx. Any of these work:

   - `nomadnet://<node>/page/index.mu`
   - `<node>:/page/index.mu`, as NomadNet writes them
   - the node's address alone (32 hexadecimal digits), for its index page

   If the bridge listens elsewhere, set `reticulum_bridge` in rynx's settings (on this computer
   only).

## What to expect

- The first page from a node can take a while: Reticulum finds a path to it, then opens an
  encrypted link. Later pages from the same node reuse the link. Over LoRa, a page can take
  seconds to minutes.
- A node that can't be reached shows a page saying so; a page rynx saw before shows its saved
  copy instead.
- Links on a page lead on through the bridge, and files are offered as downloads (`d`).
- Not yet: sending a page's fields, images, and parts of pages loaded separately.
- rynx locked to I2P only (settings) doesn't open NomadNet addresses: Reticulum traffic isn't I2P,
  unless your Reticulum setup sends it over I2P.

## Privacy and safety

- Links to nodes are anonymous: nodes learn nothing about who asks.
- The bridge listens on 127.0.0.1 only, and answers only requests carrying rynx's header, so web
  pages in other browsers can't use it to make requests.

## Testing

`test_bridge.py` runs the bridge against a NomadNet-style node over a private Reticulum instance
with no interfaces, so nothing leaves the computer. It needs `rns`, and CI runs it.

## Licence

This folder is under the MIT licence, so that it sits well beside Reticulum's own licence, which
it uses but doesn't include. Reticulum's licence doesn't allow using it in systems meant to harm
people, or to make AI training data.

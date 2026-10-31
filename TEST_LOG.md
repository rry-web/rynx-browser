# Test Log

Results of the manual checks in [TESTING.md](TESTING.md), where each ID has full steps and
the expected result. Fill in **Result** as Pass / Fail / Skipped, with the date and any notes
(for a failure: what happened, terminal app, OS version).

Rows marked Automated are covered by the tests TESTING.md names; the rest need a person.

Total checks: 162; 20 still need a person (see TESTING.md's "Still by hand"), the rest are automated.


## Automated checks

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| AUTO-01 | Test suite |  | Automated |  |  |
| AUTO-02 | Dependency audit |  | Automated |  |  |
| AUTO-03 | Live TLS checks |  | Automated |  |  |
| AUTO-06 | I2P TLS settings |  | Automated |  |  |
| AUTO-07 | Live Gemini, Gopher and feeds |  | Automated |  |  |
| AUTO-08 | Live MCP server |  | Automated |  |  |
| AUTO-04 | Extended fuzzing |  | Automated |  |  |
| AUTO-05 | CI runs on GitHub | Yes | Automated |  |  |

## General smoke test

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| SMOKE-01 | Key bindings |  | Automated |  |  |
| SMOKE-02 | Mouse |  | Automated |  |  |
| SMOKE-03 | Resizing |  | Automated |  |  |
| SMOKE-04 | Terminal restored on exit |  | Automated |  |  |

## Rendering

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| RENDER-01 | Documentation pages |  | Automated |  |  |
| RENDER-02 | Data tables |  | Automated |  |  |
| RENDER-03 | Narrow tables |  | Automated |  |  |
| RENDER-04 | Layout tables |  | Automated |  |  |
| RENDER-05 | Lists and quotes |  | Automated |  |  |
| RENDER-06 | Plain-text files |  | Automated |  |  |
| RENDER-07 | JSON |  | Automated |  |  |
| RENDER-08 | Non-text files |  | Automated |  |  |
| RENDER-11 | Site menus |  | Automated |  |  |
| RENDER-09 | Full width |  | Automated |  |  |
| RENDER-10 | No-JavaScript notices |  | Automated |  |  |

## Crashes and stability

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| CRASH-01 | Stale link selection |  | Automated |  |  |
| CRASH-02 | Tiny terminal |  | Automated |  |  |
| CRASH-03 | Visual mode on an empty page |  | Automated |  |  |
| CRASH-04 | Deeply nested lists |  | Automated |  |  |
| CRASH-05 | Extremely narrow terminal |  | Automated |  |  |
| CRASH-06 | Extremely deep nesting |  | Automated |  |  |

## Navigation and history

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| NAV-01 | Back after typing a URL |  | Automated |  |  |
| NAV-02 | Esc cancels editing |  | Automated |  |  |
| NAV-03 | Redirects update the address bar |  | Automated |  |  |
| NAV-04 | Chapter links jump within the page |  | Automated |  |  |
| NAV-05 | Back from a chapter |  | Automated |  |  |
| NAV-06 | Fragment on another page |  | Automated |  |  |
| NAV-07 | Fragment link in a new tab |  | Automated |  |  |
| NAV-08 | Redirects within a local server |  | Automated |  |  |
| NAV-09 | HTTP-only sites are not downgraded |  | Automated |  |  |
| NAV-10 | Meta refreshes and script redirects |  | Automated |  |  |

## Forms

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| FORM-01 | Search box |  | Automated |  |  |
| FORM-02 | Search in a table |  | Automated |  |  |
| FORM-03 | Checkboxes, options and choices |  | Automated |  |  |
| FORM-04 | Passwords |  | Automated |  |  |
| FORM-06 | Forms can't reach your local network |  | Automated |  |  |
| FORM-07 | Disguised links |  | Automated |  |  |
| FORM-08 | Passwords in typed addresses |  | Automated |  |  |
| FORM-05 | Typing keeps up |  | Automated |  |  |

## History and paging

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| HIST-01 | Back keeps your place |  | Automated |  |  |
| HIST-02 | Forward and reload |  | Automated |  |  |
| HIST-03 | Paging keys |  | Automated |  |  |
| HIST-04 | Links to /files/ pages |  | Automated |  |  |

## Settings and bookmarks

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| SET-01 | Settings template |  | Automated |  |  |
| SET-02 | Search engine |  | Automated |  |  |
| SET-03 | Home page |  | Automated |  |  |
| SET-04 | Bad settings |  | Automated |  |  |
| SET-05 | Bookmarks |  | Automated |  |  |
| SET-06 | Bookmarks under the Linux sandbox | Yes | Automated |  |  |
| SET-11 | User-Agent |  | Automated |  |  |
| SET-12 | Anubis | Yes |  |  |  |
| SET-13 | Copy mode |  | Automated |  |  |
| SET-14 | Settings page |  | Automated |  |  |
| SET-15 | I2P lock | Yes | Automated |  |  |
| SET-16 | Archived copy |  | Automated |  |  |
| SET-07 | Session history |  | Automated |  |  |
| SET-08 | Saved history |  | Automated |  |  |
| SET-09 | Clearing history |  | Automated |  |  |
| SET-10 | History under the Linux sandbox | Yes | Automated |  |  |

## Scrolling and input

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| SCROLL-01 | Touchpad scrolling keeps up | Yes |  |  |  |
| SCROLL-02 | Scrolling stops at the end |  | Automated |  |  |
| SCROLL-03 | j/k continue from the view |  | Automated |  |  |
| SCROLL-04 | Visual mode follows the cursor |  | Automated |  |  |
| SCROLL-05 | Wheel scrolling in visual mode |  | Automated |  |  |
| SCROLL-06 | Clicking below the page |  | Automated |  |  |
| SCROLL-07 | Quitting while moving the mouse | Yes | Automated |  |  |
| SCROLL-08 | Search survives a typo |  | Automated |  |  |
| SCROLL-09 | Search jumps |  | Automated |  |  |
| SCROLL-10 | Source view cursor |  | Automated |  |  |

## Performance and memory

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| PERF-01 | Large pages |  | Automated |  |  |
| PERF-02 | Memory use |  | Automated |  |  |
| PERF-03 | Slow pages don't freeze the UI |  | Automated |  |  |
| PERF-04 | Help page after resizing |  | Automated |  |  |

## Downloads

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| DL-01 | Speed and filename |  | Automated |  |  |
| DL-02 | Query strings |  | Automated |  |  |
| DL-03 | Error pages aren't saved |  | Automated |  |  |
| DL-04 | No silent overwrite |  | Automated |  |  |
| DL-05 | One download per tab |  | Automated |  |  |
| DL-06 | Gatekeeper checks downloads (macOS) | Yes |  |  |  |
| DL-07 | HTTP download links are upgraded |  | Automated |  |  |
| DL-08 | Resuming over I2P | Yes | Automated |  |  |

## Clipboard

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| CLIP-01 | Copy and paste still work |  | Fail | 2026-10-30 | Pasting text with line breaks into the address bar ran its lines as Enter and keys (a "t" opened a tab, so Back did nothing). Fixed after 0.3.3 (bracketed paste, CLIP-03); paste otherwise worked. |
| CLIP-02 | No clipboard available (Linux) | Yes | Automated |  |  |
| CLIP-03 | Pasted text arrives as text, not keys |  | Automated |  |  |

## Security

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| SEC-01 | Escape codes are neutralised | Yes |  |  |  |
| SEC-02 | Link destinations are shown |  | Automated |  |  |
| SEC-03 | Non-web links are refused |  | Automated |  |  |
| SEC-04 | .i2p blocked outside I2P mode |  | Automated |  |  |
| SEC-05 | Compression bomb rejected |  | Automated |  |  |
| SEC-06 | Non-UTF-8 pages still render |  | Automated |  |  |
| SEC-07 | Revoked certificates are rejected |  | Automated |  |  |
| SEC-08 | Names on your own network | Yes |  |  |  |
| SEC-09 | Staying signed in | Yes |  |  |  |
| SEC-10 | Look-alike names |  | Automated |  |  |
| SEC-11 | Cookies encrypted in the keychain | Yes |  |  |  |

## Platform security

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| PLAT-01 | Linux sandbox status | Yes | Automated |  |  |
| PLAT-02 | Linux sandbox doesn't break browsing | Yes | Automated |  |  |
| PLAT-03 | Linux download origin |  | Automated |  |  |
| PLAT-04 | Windows Mark of the Web | Yes |  |  |  |
| PLAT-05 | macOS "Where from" |  | Automated |  |  |
| PLAT-06 | Device-name downloads |  | Automated |  |  |
| PLAT-07 | Command-line options |  | Automated |  |  |
| PLAT-09 | macOS sandbox status | Yes | Pass | 2026-10-26 | macOS 27.0.1, run from the build folder |
| PLAT-10 | macOS sandbox doesn't break browsing | Yes | Partly | 2026-10-26 | HTTPS, bad certificate rejected, clipboard, download tagging checked automatically on macOS 27.0.1 |

## Releases

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| REL-01 | First beta release | Yes | Automated |  |  |
| REL-02 | Verify a downloaded build | Yes |  |  |  |
| REL-03 | Run each platform's build | Yes | Partly | 2026-10-30 | macOS (Apple Silicon): yes. Linux x86_64 (openSUSE Tumbleweed, a tester): runs and updates; not tested in depth. Others not yet. |

## Gemini, Gopher and feeds

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| SMALL-01 | Gemini |  | Automated |  |  |
| SMALL-02 | Gemini input | Yes | Automated |  |  |
| SMALL-03 | Gemini certificates |  | Automated |  |  |
| SMALL-04 | Gopher |  | Automated |  |  |
| SMALL-05 | Not in I2P mode |  | Automated |  |  |
| SMALL-06 | Feeds |  | Automated |  |  |
| SMALL-07 | Bookmarks and history |  | Automated |  |  |
| SMALL-08 | Micron (NomadNet) pages |  | Automated |  |  |
| SMALL-09 | NomadNet through the Reticulum bridge |  | Automated |  |  |

## Updates

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| UPD-01 | Updating a release build | Yes | Pass | 2026-10-28 | The released beta.6 (aarch64-apple-darwin, from GitHub) found beta.7, downloaded and verified it (U, then the button), and on the next start installed it, restarted and said "Updated to rynx 0.2.0-beta.7"; `--version` then gave beta.7. |
| UPD-02 | Cargo installs |  | Automated |  |  |
| UPD-03 | I2P |  | Automated |  |  |

## Key bindings

| ID | Check | Priority | Result | Date | Notes |
|---|---|---|---|---|---|
| KEY-01 | Changed keys in a real terminal | Yes |  |  |  |
| KEY-02 | Mistakes in keys.conf |  | Automated |  |  |

## Images

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| IMG-01 | Real images | Yes | Pass | 2026-10-30 | Kitty, macOS. Wanted: images at their real size, or larger. |
| IMG-02 | macOS Terminal |  | Pass | 2026-10-30 | Coloured blocks, blurry as expected without a graphics protocol. |
| IMG-03 | Loading settings |  | Automated |  |  |
| IMG-04 | Images in tables | Yes |  |  |  |
| IMG-05 | Sizes and galleries |  |  |  |  |
| IMG-06 | Reading in the viewer |  |  |  |  |

## Cache and snapshots

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| CACHE-01 | Back is instant |  | Automated |  |  |
| CACHE-02 | I2P site gone | Yes | Automated |  |  |
| CACHE-03 | Disk cache |  | Automated |  |  |
| CACHE-04 | Snapshots |  | Automated |  |  |
| CACHE-05 | No tracking through cache validators |  | Automated |  |  |

## Encrypted DNS

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| NET-01 | Encrypted DNS and ECH |  | Automated |  |  |
| NET-02 | Encrypted DNS on your networks | Yes |  |  |  |

## Reader mode

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| READ-01 | News article |  |  |  |  |
| READ-02 | Wikipedia |  | Automated |  |  |
| READ-03 | Stays on |  | Automated |  |  |

## Site tools (MCP)

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| MCP-01 | Listing tools |  | Automated |  |  |
| MCP-02 | Confirming a call | Yes | Automated |  |  |
| MCP-03 | Back and forward |  | Automated |  |  |
| MCP-04 | Validation |  | Automated |  |  |
| MCP-05 | Download prompt |  | Automated |  |  |

## I2P (needs a local I2P router on 127.0.0.1:4444)

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| I2P-01 | Browsing in I2P mode | Yes | Automated |  |  |
| I2P-02 | HTTPS through I2P | Yes | Automated |  |  |
| I2P-03 | Jump services | Yes | Automated |  |  |
| I2P-04 | Toggling back | Yes | Automated |  |  |
| I2P-06 | This computer in I2P mode |  | Automated |  |  |
| I2P-08 | Address helper links |  | Automated |  |  |

## I2P router (needs i2pd installed, and no other router running)

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| ROUTER-01 | Starting and stopping with rynx | Yes | Automated |  | Partly checked during development (macOS 27, i2pd 3.0.0 from Homebrew, sandbox on): rynx started it, showed "ready" and stopped it on quit; names resolved through a router started the same way after ~11 minutes. Not yet: loading a site in rynx, the settings page by eye. |
| ROUTER-02 | Kept running |  | Automated |  |  |
| ROUTER-03 | Another router |  | Automated |  |  |
| ROUTER-04 | Linux | Yes | Automated |  |  |

## The real I2P network

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| I2P-05 | Real I2P sites | Yes | Partly | 2026-10-30 | Sites listed on reg.i2p wouldn't open (address helpers looped, jump services dead): fixed in 0.3.3, to retest. |
| I2P-07 | The router's own pages |  | Partly | 2026-10-30 | i2pd's console works in I2P mode. Java I2P (console, I2PSnark, SusiMail) not installed. |

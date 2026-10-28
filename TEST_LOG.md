# Test Log

Results of the manual checks in [TESTING.md](TESTING.md), where each ID has full steps and
the expected result. Fill in **Result** as Pass / Fail / Skipped, with the date and any notes
(for a failure: what happened, terminal app, OS version).

Priority items could not be verified during development.

Total checks: 144 (31 priority).


## Automated checks

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| AUTO-01 | Test suite |  |  |  |  |
| AUTO-02 | Dependency audit |  |  |  |  |
| AUTO-03 | Live TLS checks |  |  |  |  |
| AUTO-06 | I2P TLS settings |  |  |  |  |
| AUTO-07 | Live Gemini, Gopher and feeds |  |  |  |  |
| AUTO-08 | Live MCP server |  |  |  |  |
| AUTO-04 | Extended fuzzing |  |  |  |  |
| AUTO-05 | CI runs on GitHub | Yes |  |  |  |

## General smoke test

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| SMOKE-01 | Key bindings |  |  |  |  |
| SMOKE-02 | Mouse |  |  |  |  |
| SMOKE-03 | Resizing |  |  |  |  |
| SMOKE-04 | Terminal restored on exit |  |  |  |  |

## Rendering

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| RENDER-01 | Documentation pages |  |  |  |  |
| RENDER-02 | Data tables |  |  |  |  |
| RENDER-03 | Narrow tables |  |  |  |  |
| RENDER-04 | Layout tables |  |  |  |  |
| RENDER-05 | Lists and quotes |  |  |  |  |
| RENDER-06 | Plain-text files |  |  |  |  |
| RENDER-07 | JSON |  |  |  |  |
| RENDER-08 | Non-text files |  |  |  |  |
| RENDER-11 | Site menus |  |  |  |  |
| RENDER-09 | Full width |  |  |  |  |
| RENDER-10 | No-JavaScript notices |  |  |  |  |

## Crashes and stability

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| CRASH-01 | Stale link selection |  |  |  |  |
| CRASH-02 | Tiny terminal |  |  |  |  |
| CRASH-03 | Visual mode on an empty page |  |  |  |  |
| CRASH-04 | Deeply nested lists |  |  |  |  |
| CRASH-05 | Extremely narrow terminal |  |  |  |  |
| CRASH-06 | Extremely deep nesting |  |  |  |  |

## Navigation and history

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| NAV-01 | Back after typing a URL |  |  |  |  |
| NAV-02 | Esc cancels editing |  |  |  |  |
| NAV-03 | Redirects update the address bar |  |  |  |  |
| NAV-04 | Chapter links jump within the page |  |  |  |  |
| NAV-05 | Back from a chapter |  |  |  |  |
| NAV-06 | Fragment on another page |  |  |  |  |
| NAV-07 | Fragment link in a new tab |  |  |  |  |
| NAV-08 | Redirects within a local server |  |  |  |  |
| NAV-09 | HTTP-only sites are not downgraded |  |  |  |  |

## Forms

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| FORM-01 | Search box |  |  |  |  |
| FORM-02 | Search in a table |  |  |  |  |
| FORM-03 | Checkboxes, options and choices |  |  |  |  |
| FORM-04 | Passwords |  |  |  |  |
| FORM-06 | Forms can't reach your local network |  |  |  |  |
| FORM-07 | Disguised links |  |  |  |  |
| FORM-08 | Passwords in typed addresses |  |  |  |  |
| FORM-05 | Typing keeps up |  |  |  |  |

## History and paging

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| HIST-01 | Back keeps your place |  |  |  |  |
| HIST-02 | Forward and reload |  |  |  |  |
| HIST-03 | Paging keys |  |  |  |  |
| HIST-04 | Links to /files/ pages |  |  |  |  |

## Settings and bookmarks

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| SET-01 | Settings template |  |  |  |  |
| SET-02 | Search engine |  |  |  |  |
| SET-03 | Home page |  |  |  |  |
| SET-04 | Bad settings |  |  |  |  |
| SET-05 | Bookmarks |  |  |  |  |
| SET-06 | Bookmarks under the Linux sandbox | Yes |  |  |  |
| SET-11 | User-Agent |  |  |  |  |
| SET-12 | Anubis | Yes |  |  |  |
| SET-13 | Copy mode |  |  |  |  |
| SET-14 | Settings page |  |  |  |  |
| SET-15 | I2P lock | Yes |  |  |  |
| SET-16 | Archived copy |  |  |  |  |
| SET-07 | Session history |  |  |  |  |
| SET-08 | Saved history |  |  |  |  |
| SET-09 | Clearing history |  |  |  |  |
| SET-10 | History under the Linux sandbox | Yes |  |  |  |

## Scrolling and input

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| SCROLL-01 | Touchpad scrolling keeps up | Yes |  |  |  |
| SCROLL-02 | Scrolling stops at the end |  |  |  |  |
| SCROLL-03 | j/k continue from the view |  |  |  |  |
| SCROLL-04 | Visual mode follows the cursor |  |  |  |  |
| SCROLL-05 | Wheel scrolling in visual mode |  |  |  |  |
| SCROLL-06 | Clicking below the page |  |  |  |  |
| SCROLL-07 | Quitting while moving the mouse | Yes |  |  |  |
| SCROLL-08 | Search survives a typo |  |  |  |  |
| SCROLL-09 | Search jumps |  |  |  |  |
| SCROLL-10 | Source view cursor |  |  |  |  |

## Performance and memory

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| PERF-01 | Large pages |  |  |  |  |
| PERF-02 | Memory use |  |  |  |  |
| PERF-03 | Slow pages don't freeze the UI |  |  |  |  |
| PERF-04 | Help page after resizing |  |  |  |  |

## Downloads

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| DL-01 | Speed and filename |  |  |  |  |
| DL-02 | Query strings |  |  |  |  |
| DL-03 | Error pages aren't saved |  |  |  |  |
| DL-04 | No silent overwrite |  |  |  |  |
| DL-05 | One download per tab |  |  |  |  |
| DL-06 | Gatekeeper checks downloads (macOS) | Yes |  |  |  |
| DL-07 | HTTP download links are upgraded |  |  |  |  |
| DL-08 | Resuming over I2P | Yes |  |  |  |

## Clipboard

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| CLIP-01 | Copy and paste still work |  |  |  |  |
| CLIP-02 | No clipboard available (Linux) | Yes |  |  |  |

## Security

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| SEC-01 | Escape codes are neutralised | Yes |  |  |  |
| SEC-02 | Link destinations are shown |  |  |  |  |
| SEC-03 | Non-web links are refused |  |  |  |  |
| SEC-04 | .i2p blocked outside I2P mode |  |  |  |  |
| SEC-05 | Compression bomb rejected |  |  |  |  |
| SEC-06 | Non-UTF-8 pages still render |  |  |  |  |
| SEC-07 | Revoked certificates are rejected |  |  |  |  |

## Platform security

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| PLAT-01 | Linux sandbox status | Yes |  |  |  |
| PLAT-02 | Linux sandbox doesn't break browsing | Yes |  |  |  |
| PLAT-03 | Linux download origin |  |  |  |  |
| PLAT-04 | Windows Mark of the Web | Yes |  |  |  |
| PLAT-05 | macOS "Where from" |  |  |  |  |
| PLAT-06 | Device-name downloads |  |  |  |  |
| PLAT-07 | Command-line options |  |  |  |  |
| PLAT-09 | macOS sandbox status | Yes | Pass | 2026-10-26 | macOS 27.0.1, run from the build folder |
| PLAT-10 | macOS sandbox doesn't break browsing | Yes | Partly | 2026-10-26 | HTTPS, bad certificate rejected, clipboard, download tagging checked automatically on macOS 27.0.1 |

## Releases

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| REL-01 | First beta release | Yes |  |  |  |
| REL-02 | Verify a downloaded build | Yes |  |  |  |
| REL-03 | Run each platform's build | Yes |  |  |  |

## Gemini, Gopher and feeds

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| SMALL-01 | Gemini |  |  |  |  |
| SMALL-02 | Gemini input | Yes |  |  |  |
| SMALL-03 | Gemini certificates |  |  |  |  |
| SMALL-04 | Gopher |  |  |  |  |
| SMALL-05 | Not in I2P mode |  |  |  |  |
| SMALL-06 | Feeds |  |  |  |  |
| SMALL-07 | Bookmarks and history |  |  |  |  |

## Updates

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| UPD-01 | Updating a release build | Yes |  |  |  |
| UPD-02 | Cargo installs |  |  |  |  |
| UPD-03 | I2P |  |  |  |  |

## Key bindings

| ID | Check | Priority | Result | Date | Notes |
|---|---|---|---|---|---|
| KEY-01 | Changed keys in a real terminal | Yes |  |  |  |
| KEY-02 | Mistakes in keys.conf |  |  |  |  |

## Images

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| IMG-01 | Real images | Yes |  |  |  |
| IMG-02 | macOS Terminal |  |  |  |  |
| IMG-03 | Loading settings |  |  |  |  |

## Cache and snapshots

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| CACHE-01 | Back is instant |  |  |  |  |
| CACHE-02 | I2P site gone | Yes |  |  |  |
| CACHE-03 | Disk cache |  |  |  |  |
| CACHE-04 | Snapshots |  |  |  |  |

## Reader mode

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| READ-01 | News article |  |  |  |  |
| READ-02 | Wikipedia |  |  |  |  |
| READ-03 | Stays on |  |  |  |  |

## Site tools (MCP)

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| MCP-01 | Listing tools |  |  |  |  |
| MCP-02 | Confirming a call | Yes |  |  |  |
| MCP-03 | Back and forward |  |  |  |  |
| MCP-04 | Validation |  |  |  |  |
| MCP-05 | Download prompt |  |  |  |  |

## I2P (needs a local I2P router on 127.0.0.1:4444)

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| I2P-01 | Browsing in I2P mode | Yes |  |  |  |
| I2P-02 | HTTPS through I2P | Yes |  |  |  |
| I2P-03 | Jump services | Yes |  |  |  |
| I2P-04 | Toggling back | Yes |  |  |  |

## I2P router (needs i2pd installed, and no other router running)

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| ROUTER-01 | Starting and stopping with rynx | Yes |  |  | Partly checked during development (macOS 27, i2pd 3.0.0 from Homebrew, sandbox on): rynx started it, showed "ready" and stopped it on quit; names resolved through a router started the same way after ~11 minutes. Not yet: loading a site in rynx, the settings page by eye. |
| ROUTER-02 | Kept running |  |  |  |  |
| ROUTER-03 | Another router |  |  |  |  |
| ROUTER-04 | Linux | Yes |  |  |  |

# Test Log

Results of the manual checks in [TESTING.md](TESTING.md), where each ID has full steps and
the expected result. Fill in **Result** as Pass / Fail / Skipped, with the date and any notes
(for a failure: what happened, terminal app, OS version).

Priority items could not be verified during development.

Total checks: 55 (10 priority).


## Automated checks

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| AUTO-01 | Test suite |  |  |  |  |
| AUTO-02 | Dependency audit |  |  |  |  |
| AUTO-03 | Live TLS checks |  |  |  |  |
| AUTO-04 | Extended fuzzing |  |  |  |  |
| AUTO-05 | CI runs on GitHub | Yes |  |  |  |

## General smoke test

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| SMOKE-01 | Key bindings |  |  |  |  |
| SMOKE-02 | Mouse |  |  |  |  |
| SMOKE-03 | Resizing |  |  |  |  |
| SMOKE-04 | Terminal restored on exit |  |  |  |  |

## Crashes and stability

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| CRASH-01 | Stale link selection |  |  |  |  |
| CRASH-02 | Tiny terminal |  |  |  |  |
| CRASH-03 | Visual mode on an empty page |  |  |  |  |
| CRASH-04 | Deeply nested lists |  |  |  |  |
| CRASH-05 | Extremely narrow terminal |  |  |  |  |

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

## I2P (needs a local I2P router on 127.0.0.1:4444)

| ID | Check | Priority | Result | Date | Notes |
|----|-------|:--------:|--------|------|-------|
| I2P-01 | Browsing in I2P mode | Yes |  |  |  |
| I2P-02 | HTTPS through I2P | Yes |  |  |  |
| I2P-03 | Jump services | Yes |  |  |  |
| I2P-04 | Toggling back | Yes |  |  |  |

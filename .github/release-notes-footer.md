
## Installing

Download the archive for your platform below, extract it, and run `rynx` from a terminal.

| Platform | Archive |
|---|---|
| Linux x86_64 | `x86_64-unknown-linux-gnu` (or `x86_64-unknown-linux-musl`, statically linked, for any distribution) |
| Linux ARM64 | `aarch64-unknown-linux-gnu` |
| macOS Apple Silicon | `aarch64-apple-darwin` |
| macOS Intel | `x86_64-apple-darwin` |
| Windows x86_64 | `x86_64-pc-windows-msvc` |

The binaries are **not code-signed**, so the OS will warn the first time:

- **macOS:** Gatekeeper blocks unsigned downloads. After checking the file, allow it with
  `xattr -d com.apple.quarantine rynx`, or right-click it in Finder and choose Open.
- **Windows:** SmartScreen may show "Windows protected your PC"; choose *More info* → *Run anyway*.

## Verifying

Every archive is listed in `SHA256SUMS`, and has a build provenance attestation proving it was built
from this repository by GitHub Actions:

```sh
sha256sum -c SHA256SUMS --ignore-missing          # macOS: shasum -a 256 -c SHA256SUMS
gh attestation verify <archive> --repo rry-web/rynx-browser
```

## Platform notes

- **Linux:** the browser sandboxes itself with Landlock (kernel 5.13 or newer): it can only write to
  your Downloads folder and can't read the rest of your home directory. The status line shows
  whether the sandbox is active; `--no-sandbox` turns it off for troubleshooting.
- **All platforms:** downloads are tagged as coming from the internet (macOS quarantine, Windows
  Mark of the Web, Linux `user.xdg.origin.url`), so the OS checks them before they're opened.

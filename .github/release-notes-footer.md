
## Installing

Download the archive for your platform below, extract it, and run `rynx` from a terminal. From
then on rynx can update itself: it says when a new version is out, and `U` installs it. (The
files named just `rynx-<version>-<platform>` are the bare programs the updater downloads.)

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

Every file is listed in `SHA256SUMS`, which is signed with the release key
(`SHA256SUMS.sig`, Ed25519; rynx's updater refuses anything it doesn't vouch for), and has a build
provenance attestation proving it was built from this repository by GitHub Actions:

```sh
# The signature, with the public key from the repository (.github/release-signing-key.pub.pem;
# in an emergency a release may be signed with the backup key, release-signing-key-backup.pub.pem)
openssl pkeyutl -verify -pubin -inkey release-signing-key.pub.pem -rawin -in SHA256SUMS -sigfile SHA256SUMS.sig
sha256sum -c SHA256SUMS --ignore-missing          # macOS: shasum -a 256 -c SHA256SUMS
gh attestation verify <archive> --repo rry-web/rynx-browser
```

Each program also carries its exact list of dependencies, so you can check a copy for known
vulnerabilities at any time: `cargo install cargo-audit`, then `cargo audit bin rynx`.

## Platform notes

- **Linux and macOS:** the browser sandboxes itself (Landlock on kernel 5.13 or newer; Seatbelt on
  macOS): it can only write to your Downloads, settings and cache folders and can't read the rest
  of your home directory. The status line shows
  whether the sandbox is active; `--no-sandbox` turns it off for troubleshooting.
- **All platforms:** downloads are tagged as coming from the internet (macOS quarantine, Windows
  Mark of the Web, Linux `user.xdg.origin.url`), so the OS checks them before they're opened.

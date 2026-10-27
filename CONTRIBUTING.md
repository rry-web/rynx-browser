# Contributing to rynx

Thanks for helping. Bug reports, ideas and pull requests are all welcome; for a security issue,
see [SECURITY.md](SECURITY.md) instead of opening a public issue.

## Building and checking

```sh
cargo run                    # a development build
scripts/check.sh --quick     # formatting, lints and tests: run before every commit
scripts/check.sh             # everything CI checks, including cargo deny and the oldest Rust
```

`scripts/check.sh` runs what CI runs, so a change that passes it passes CI. It needs
`cargo install cargo-deny --locked` and `rustup toolchain install 1.90 --profile minimal` for
the last two steps (it skips them, saying so, if they're missing).

More tests:

```sh
cargo test -- --ignored                 # tests that need the internet (TLS servers, real sites)
FUZZ_ITERATIONS=100000 cargo test --release --test fuzz_render
cargo +nightly fuzz run render -- -max_total_time=60    # coverage-guided fuzzing: fuzz/README.md
cargo run --example render_file -- page.html 100        # see how rynx lays out a saved page
cargo llvm-cov --open        # test coverage, in the browser (needs cargo-llvm-cov and llvm-tools-preview)
```

If another Rust (Homebrew's, say) comes first on PATH, run rustup's tools by path, as
`scripts/check.sh` does: `PATH="$(dirname "$(rustup which cargo)"):$PATH" cargo llvm-cov`.

[TESTING.md](TESTING.md) lists the checks that need a person (terminals, platforms, I2P), with
results in [TEST_LOG.md](TEST_LOG.md).

## Changes

- **Code style:** `cargo fmt` (120 columns, `rustfmt.toml`) and no clippy warnings. Comments say
  why, in plain sentences; names say what.
- **Tests:** a fix comes with a test that fails without it; a feature with tests of what it must
  and mustn't do. Anything that reads untrusted input (pages, feeds, protocols, files) is worth a
  fuzz target in `fuzz/`.
- **Security:** rynx's promises (no JavaScript, no tracking, HTTPS, the sandbox, nothing sent to
  the local network from public pages) are listed in the README; a change must keep them. Pages
  the browser makes itself (`about:`) must stay out of reach of web pages' links and forms.
- **Changelog:** add a line under `## [Unreleased]` in [CHANGELOG.md](CHANGELOG.md), saying
  what changed for the user.
- **Commits:** a short summary line, then what changed and why.

## Releases

Maintainers release with one command, on an up-to-date `main` whose changes are listed under
`## [Unreleased]`:

```sh
scripts/release.sh 0.2.0-beta.7
```

It sets the version, runs the checks, pushes, waits for CI, and tags; the release workflow then
builds every platform, signs the checksums with the release key, and publishes the GitHub release
and the crate (trusted publishing). Tags with a `-` are pre-releases.

## Automation

| Workflow | When | What |
| :--- | :--- | :--- |
| `ci.yml` | every push and pull request, and weekly | tests (Linux, macOS, Windows), lints, renderer fuzzing, cargo deny, Rust 1.90, test coverage (on the run's summary page; below 80% fails) |
| `fuzz.yml` | nightly | coverage-guided fuzzing of every target, keeping their corpora |
| `live.yml` | weekly | the tests that need the internet |
| `release.yml` | a `v*` tag | checks, builds, signing, the GitHub release, crates.io |

Dependabot proposes dependency and action updates weekly. Actions are pinned to commit hashes;
update them deliberately, checking what changed.

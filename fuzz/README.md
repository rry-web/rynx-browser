# Fuzzing

Coverage-guided fuzzing (libFuzzer, through `cargo fuzz`) of everything rynx reads from the
network or disk. It needs nightly Rust and `cargo install cargo-fuzz`.

```sh
cargo +nightly fuzz list                         # the targets
cargo +nightly fuzz run render -- -max_total_time=60
cargo +nightly fuzz run render fuzz/artifacts/render/crash-...   # replay a crash
```

| Target | What it reads | What must hold |
| :--- | :--- | :--- |
| `render` | any page, whole and in reader mode | no crash; links and jump targets on real lines |
| `gemtext` | Gemini pages | no crash |
| `gopher` | menus and addresses | selectors never carry line breaks |
| `feed` | RSS and Atom | no crash; no entity expansion |
| `anubis` | bot-check pages | the answer only goes back to the page's own site |
| `cookies` | Set-Cookie headers | nothing is sent to another site |
| `settings` | settings files | written back and read again, nothing changes |
| `update_list` | the release list, checksum lists | only newer releases, with this platform's build |

CI runs every target nightly (`.github/workflows/fuzz.yml`) and keeps each one's corpus between
runs; a crash fails the run and uploads the input that caused it. To keep a crash as a test,
copy its input into `fuzz/regressions/<target>/` and it's replayed on every nightly run.

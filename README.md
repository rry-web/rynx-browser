# RYNX BROWSER

A terminal web browser written in Rust, loosely inspired by **Lynx** and **w3m**, with absolutely
**zero JavaScript**.

- **Readable pages:** headings, numbered and nested lists, blockquotes, aligned table columns, and
  preformatted code, laid out for the terminal. Site menus are shown compactly on one line
  (`Home · News · Sport`) instead of a long column of links. Plain-text files (`.txt`, JSON, source code) are
  shown as text with clickable URLs; images, PDFs and other files are offered for download.
- **Forms:** search boxes and other forms work: text fields, checkboxes, choices and buttons, sent
  with GET or POST.
- **Tabs, history and search:** Back and Forward return you to where you were on each page,
  in-page links jump without reloading, and `/` finds text on the page.
- **Fast:** pages are laid out on a background thread, so even huge or slow pages never freeze the
  screen.
- **Private and hardened:** no cookies, no JavaScript, HTTPS everywhere, and on Linux a filesystem
  sandbox. See [Security and privacy](#security-and-privacy).
- **I2P:** browse `.i2p` sites through a local I2P router.

## Installation

Pre-built binaries for Linux, macOS and Windows are on the
[releases page](https://github.com/rry-web/rynx-browser/releases), including beta builds. Each
release explains how to verify the download and get past the warnings shown for unsigned binaries.

To build from source, install [Rust](https://rustup.rs) and run:

```sh
cargo run --release
```

Command-line options:

| Option | Effect |
| :--- | :--- |
| `--version`, `-V` | Print the version |
| `--help`, `-h` | Print usage |
| `--no-sandbox` | Turn off the Linux filesystem sandbox (for troubleshooting) |

## Using it

Press **`e`**, type an address (or search words) and press Enter. Words that aren't an address are
searched with [Marginalia](https://marginalia-search.com), a search engine for the
non-commercial web. Marginalia sometimes shows a "wait a moment" page to slow down bots; select its
"proceed manually" link to continue, or set a different search engine in [settings](#settings).

Press **`Tab`** to step through the links and form fields on a page. The bottom border always shows
where the selected link goes, or what a form field is and where its form sends what you type.

To use a form, select a field and press **Enter** to start typing, then **Enter** again to submit
(or **Esc** to stop typing). There are no cookies, so sites that need you to log in won't keep
you signed in.

Downloads (**`d`** on a link) are saved to your Downloads folder after you confirm.

Press **`b`** to bookmark a page (again to remove it) and **`B`** to see your bookmarks.

## Settings

The first run creates `settings.conf` in your config folder (`~/.config/rynx-browser` on Linux,
`~/Library/Application Support/rynx-browser` on macOS, `%APPDATA%\rynx-browser\config` on Windows),
with every option commented out. Remove the `#` to use one:

```ini
home_page = https://en.wikipedia.org/
search_url = https://html.duckduckgo.com/html/?q={query}
i2p_proxy = http://127.0.0.1:4444
download_dir = /path/to/folder
```

Bookmarks are kept next to it in `bookmarks.txt`.

## Key bindings

The same list is on the start page.

### Moving around
| Key | Action |
| :--- | :--- |
| **`h` `j` `k` `l`** | Move the cursor (vim-style); the view follows |
| **`Up` / `Down`**, scroll wheel | Scroll; the cursor stays on screen |
| **`PageUp` / `PageDown`**, **`Ctrl+b` / `Ctrl+f`** | Scroll a screen |
| **`Ctrl+u` / `Ctrl+d`** | Scroll half a screen |
| **`g` / `G`**, **`Home` / `End`** | Top / bottom of the page |

### Links and history
| Key | Action |
| :--- | :--- |
| **`Tab` / `Shift+Tab`** | Select the next / previous link or form field |
| **`Enter`**, click | Open the selected link, or use the form field |
| **`Backspace`**, **`Left`**, **`H`** | Back, to where you were on the page |
| **`Right`**, **`L`** | Forward |
| **`r`** | Reload, keeping your place |
| **`b`** / **`B`** | Bookmark the page (or remove it) / show bookmarks |
| **`d`** | Download the selected link |
| **`Esc`** | Clear a finished or failed download |

### Forms
| Key | Action |
| :--- | :--- |
| **`Enter`** on a text field | Start typing |
| **`Enter`** while typing | Submit the form (in a text area: new line) |
| **`Esc`** while typing | Stop typing, keeping the text |
| **`Ctrl+u`** / **`Ctrl+v`** while typing | Clear the field / paste |
| **`Enter`** on a checkbox, option or choice | Tick it, pick it, or show the next choice |
| **`Enter`** on a button | Submit the form |

### Tabs
| Key | Action |
| :--- | :--- |
| **`n`** / **`w`** | New tab / close tab |
| **`t`**, **`Ctrl`+click** | Open the selected link in a new tab |
| **`[`** / **`]`** | Previous / next tab |

### Address bar (press `e`)
| Key | Action |
| :--- | :--- |
| **`Enter`** | Go (or search, if it isn't an address) |
| **`Esc`** | Cancel and restore the address |
| **`Ctrl+u`** / **`Ctrl+k`** | Clear / clear and paste |
| **`Ctrl+y`** / **`Ctrl+v`** | Copy / paste |

### Finding text, copying, and more
| Key | Action |
| :--- | :--- |
| **`/`** | Find on the page (live highlighting) |
| **`>`** / **`<`** | Next / previous match |
| **`v`**, then **`y`** | Select text with the cursor keys, then copy it |
| **`Shift+V`** | Show the page source |
| **`p`** | Toggle I2P mode (through the proxy at `127.0.0.1:4444`) |
| **`q`** | Quit |

## Security and privacy

A text browser without JavaScript already avoids the biggest source of browser exploits. On top of
that:

- **No tracking:** no cookies, no storage, no `Referer` header, no JavaScript.
- **HTTPS everywhere:** addresses, redirects, downloads and form submissions are upgraded to HTTPS
  (except `localhost` and `.i2p` sites). Certificates are checked by your operating system,
  including revocation on macOS and Windows, with TLS 1.2 or newer.
- **Linux sandbox:** the browser restricts itself with Landlock (kernel 5.13+): it can write only
  to your Downloads folder and its settings folder, and can't read the rest of your home directory, so a bug can't be used
  to read SSH keys or plant files. The status line shows whether it's active.
- **Hostile pages are contained:** escape sequences that could take over your terminal are
  stripped; oversized, deeply nested or compressed "bomb" pages can't freeze or crash the browser;
  public pages can't redirect into your local network.
- **Careful with links and forms:** the destination of every link and form is shown before you
  use it. `mailto:`, `javascript:` and `file:` links are refused, and so are links and forms that
  disguise their real site with a user name (`https://paypal.com@evil.example/`). A form is never
  allowed to put a password in a web address, and a public page's form can't send data to your
  router or other devices on your local network. Passwords typed into the address bar are hidden
  on screen and never saved in bookmarks.
- **Safe downloads:** files are tagged as coming from the internet (macOS quarantine, Windows Mark
  of the Web, Linux `user.xdg.origin.url`) so your OS checks them before they run; filenames are
  made safe on every OS, and existing files are never overwritten silently.
- **I2P:** in I2P mode all traffic goes through the I2P proxy, and `.i2p` addresses are never looked
  up outside it.
- **Maintained dependencies:** `cargo audit` runs in CI on every change and weekly.

## Development

```sh
cargo test                                   # unit, integration and randomised renderer tests
cargo test --test live_tls -- --ignored      # TLS checks against public test servers
FUZZ_ITERATIONS=100000 cargo test --release --test fuzz_render
```

[TESTING.md](TESTING.md) has a checklist of hands-on checks (terminals, touchpads, platforms, I2P)
with results recorded in [TEST_LOG.md](TEST_LOG.md). Changes are listed in
[CHANGELOG.md](CHANGELOG.md).

## Roadmap
- [x] **I2P integration** through a local proxy
- [x] **Downloads** with progress, safe filenames and OS security tagging
- [x] **Clipboard** support for the address bar and page text
- [x] **Find on page** with highlighting
- [x] **Rendering** of tables, lists, quotes, code and plain-text files
- [x] **Forms** for searching and other text input
- [ ] **Images** with `ratatui-image` (Sixel/Kitty)
- [x] **Bookmarks**
- [x] **Settings file** (home page, search engine, I2P proxy, downloads folder)
- [ ] **Persistent history** (opt-in)
- [ ] **Gopher, Gemini and RSS** support
- [ ] **MCP support** as an alternative to JavaScript

If you want to contribute, open an issue or pull request on GitHub.

## License

This project is licensed under GPLv3.

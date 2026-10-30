# Testing

Almost everything here is checked by scripts: `cargo test` runs them, CI runs them on Linux,
macOS and Windows for every push, and weekly against real sites. The **terminal tests**
(`tests/terminal/`) run the real program in a pseudo-terminal, typing and clicking as a person
would and reading the screen back, against local web, Gemini, Gopher and MCP servers and a fake
I2P proxy; each is named after the check below it covers.

What's left by hand is what needs a particular terminal, operating system, website, account or
network. Record those results in [TEST_LOG.md](TEST_LOG.md).

```sh
cargo test                                   # everything, including the terminal tests (about 30 s)
cargo test --test terminal -- smoke_01       # one check, by its ID
cargo test -- --ignored                      # the ones that need the internet (live sites, TLS, Gemini...)
```

## Still by hand

Items marked **(priority)** haven't been checked by a person yet.

### Settings and bookmarks
- [ ] **SET-12 (priority) Anubis.** Open a site behind Anubis (Royal Road has moved to
      Cloudflare; Anubis's documentation lists sites using it) and a second page there.
      *Expected:* "Passing the site's bot check (Anubis), without JavaScript..." for a moment,
      then the real page (not "Making sure you're not a bot"). Later pages on the site load
      straight away. With `anubis = off`, the check's page shows and the status line says
      passing is turned off. Every method (fast, sha256, argon2id, hashx, metarefresh) has only
      been tested against a simulated Anubis.

### Scrolling and input
- [ ] **SCROLL-01 (priority) Touchpad scrolling keeps up.** On the Moby Dick page, scroll
      hard with the touchpad, then stop.
      *Expected:* scrolling stops when your fingers stop. (A short glide can be macOS
      momentum scrolling, which the terminal sends itself.)

### Downloads
- [ ] **DL-06 (priority) Gatekeeper checks downloads (macOS).** Download a small unsigned app
      or script (e.g. a `.zip` containing a `.app` from a GitHub release), then open it from
      Finder.
      *Expected:* macOS shows its "downloaded from the internet" warning.
      `xattr -l ~/Downloads/<file>` shows `com.apple.quarantine: 0081;...;rynx;`.
      *Automated already:* the quarantine attribute is checked automatically (downloads::dl_06...);
      macOS's warning when opening needs a person.

### Clipboard
- [ ] **CLIP-01 Copy and paste still work.** In Edit mode: `Ctrl+y` copies the address,
      `Ctrl+v` pastes, `Ctrl+k` replaces the address with the clipboard. In visual mode, `y`
      copies the selection.

### Security
- [ ] **SEC-01 (priority) Escape codes are neutralised.** With the test servers running, open
      `http://localhost:8000/evil.html`. Best done in iTerm2, kitty or WezTerm, which honour
      clipboard escape codes.
      *Expected:* the window title doesn't change to "TITLE CHANGED", the screen isn't
      cleared, and your clipboard is **not** replaced with "pwned". The page shows
      `Before]52;c;cHduZWQ= after[2J`, i.e. the escape characters removed and the rest as
      plain text.
      *Automated already:* automated through a terminal emulator (security::sec_01...); iTerm2,
      kitty or WezTerm themselves are worth a look.
- [ ] **SEC-08 (priority) Names on your own network.** Open your router's or another device's
      local name (e.g. `http://router.lan/`, or a `.local` name), then `http://localtest.me/` (a
      public name that points at 127.0.0.1).
      *Expected:* the local name opens; localtest.me is refused with "points at 127.0.0.1 (or ::1),
      on this computer or your local network" and the hint about `local_names`.
      *Automated already:* localtest.me is refused automatically (tests/dns.rs, live); a real local
      name needs your network.
- [ ] **SEC-09 (priority) Staying signed in.** On a site you have an account with, press `C` on
      its sign-in page, sign in, and follow a few links; quit and start rynx again.
      *Expected:* you stay signed in across pages, and after the restart if the site's sign-in
      offers "remember me". `about:cookies` lists the site; Forget there (or `C` on its page)
      signs you out. Without `C`, the sign-in page's status line says to press it first.
- [ ] **SEC-11 (priority) Cookies encrypted in the keychain.** After SEC-09, look at
      `about:cookies`, the settings folder, and (macOS) Keychain Access, searching "rynx-browser".
      *Expected:* before any site keeps cookies, macOS doesn't ask about the keychain at all; after
      SEC-09 and a restart, the page says "encrypted, with a key in the system keychain"; the folder has
      `cookies.enc` and no `cookies.json`; the keychain has a "rynx-browser" entry. Start rynx
      again: still signed in. On Linux, the same with GNOME Keyring or KWallet running; without
      one, the page says they're not encrypted and why.
      *Automated already:* Linux is automated (CI's linux-system job); macOS's Keychain and its
      prompt need a person.

### Platform security
- [ ] **PLAT-10 (priority) macOS sandbox doesn't break browsing.** With the sandbox active, browse
      a few HTTPS sites, download a file, bookmark a page, copy text with `v`/`y`, and open a
      Gemini site.
      *Expected:* all of it works. Run once from `cargo install` (in `~/.cargo/bin`) and once
      from an unpacked release archive. If anything fails, note the macOS version and retry with
      `--no-sandbox`. `cargo test --test sandbox -- --ignored` checks HTTPS, certificate
      rejection, the clipboard and download tagging inside the sandbox automatically.
      *Automated already:* the suite runs sandboxed; a cargo install and a release archive need a
      person.
- [ ] **PLAT-04 (priority) Windows Mark of the Web.** On Windows, download a file, then in
      PowerShell run `Get-Content <file> -Stream Zone.Identifier`.
      *Expected:* it shows `ZoneId=3` and the download URL; opening a downloaded program
      triggers a SmartScreen check.

### Releases
- [ ] **REL-02 (priority) Verify a downloaded build.** Download your platform's archive and
      `SHA256SUMS`, then run `sha256sum -c SHA256SUMS --ignore-missing` and
      `gh attestation verify <archive> --repo rry-web/rynx-browser`.
      *Expected:* the checksum matches and the attestation verifies.
- [ ] **REL-03 (priority) Run each platform's build.** On each OS you have, extract and run the
      binary, following the release notes for the unsigned-binary warning.
      *Expected:* it starts and browses normally.

### Key bindings
- [ ] **KEY-01 (priority) Changed keys in a real terminal.** In `keys.conf` (settings folder),
      add `reload = F5`, `next_tab = Alt+l` and `quit = Ctrl+q only`, restart, and open two tabs.
      *Expected:* F5 reloads, Alt+l moves to the next tab, `q` does nothing and Ctrl+q quits.
      `about:keys` lists them. (macOS Terminal sends Alt only with "Use Option as Meta key" on,
      in Settings, Profiles, Keyboard.)
      *Automated already:* the keys are automated (settings::key_01_changed_keys); macOS Terminal's
      Option-as-Meta setting is yours to check.

### Images
- [ ] **IMG-01 (priority) Real images.** In iTerm2, Kitty, Ghostty or WezTerm, open a page with
      pictures (a Wikipedia article) and press `I`.
      *Expected:* "Loading N images...", then the pictures appear in the page under their
      captions, sharp, not as blocks. Scrolling keeps them in place; `i` on one shows it full
      screen, and Esc returns. The settings page says which method it draws with.
- [ ] **IMG-02 macOS Terminal.** The same in macOS Terminal.
      *Expected:* rynx starts normally (it doesn't ask this terminal about images), and pictures
      appear as rough coloured blocks.

### Reader mode
- [ ] **READ-01 News article.** Open an article on a news site (BBC News, Ars Technica) and press
      `R`.
      *Expected:* the headline, byline and story only; no site menus, related stories, share
      buttons or comments. The address bar shows READER. Each picture's description appears once.

### The real I2P network

- [ ] **I2P-05 (priority) Real I2P sites.** With i2pd installed, tick "Start i2pd when rynx
      starts" in settings, restart, wait for "The I2P router is ready", then after 5 to 10
      minutes press `p` and open `http://stats.i2p/`, and an `https://` clearweb site.
      *Expected:* both load (the second through the router's outproxy); `pgrep i2pd` finds
      nothing after quitting.
      *Automated already:* everything about I2P mode against a fake proxy (`i2p::*`), the router
      started and stopped with the real i2pd (`tests/i2pd.rs`, CI's Linux job too); only the
      real network needs a person.
- [ ] **I2P-07 The router's own pages.** Open the i2pd console (`127.0.0.1:7070`) in I2P mode by
      typing it. With Java I2P running instead, open its console (`127.0.0.1:7657`), I2PSnark
      (`/i2psnark/`) and SusiMail (`/susimail/`), all linked from `about:i2p`.
      *Expected:* each is readable and its forms work without JavaScript: in I2PSnark, adding a
      magnet link (copied by opening one on a page) and seeing its progress; in SusiMail,
      signing in and reading a message. Anything that needs JavaScript is noted here.
      *Automated already:* this computer reached directly in I2P mode while sites' links to it
      are refused (`i2p::i2p_06_*`), magnet links copied, and `about:i2p`
      (`integration_test`).

## Automated

Each check and the test that covers it (`tests/terminal/` unless named otherwise).

| Check | What | Test |
|---|---|---|
| AUTO-01 | Test suite | CI (`.github/workflows/`) |
| AUTO-02 | Dependency audit | CI (`.github/workflows/`) |
| AUTO-03 | Live TLS checks | CI (`.github/workflows/`) |
| AUTO-06 | I2P TLS settings | CI (`.github/workflows/`) |
| AUTO-07 | Live Gemini, Gopher and feeds | CI (`.github/workflows/`) |
| AUTO-08 | Live MCP server | CI (`.github/workflows/`) |
| AUTO-04 | Extended fuzzing | CI (`.github/workflows/`) |
| AUTO-05 | CI runs on GitHub | CI (`.github/workflows/`) |
| SMOKE-01 | Key bindings | browsing::smoke_01_key_bindings |
| SMOKE-02 | Mouse | browsing::smoke_02_mouse |
| SMOKE-03 | Resizing | browsing::smoke_03_resizing_rewraps |
| SMOKE-04 | Terminal restored on exit | main::it_starts_sandboxed_shows_the_start_page_and_quits_cleanly |
| RENDER-01 | Documentation pages | pages::render_01_code; tests/live_sites::render_01_rust_documentation |
| RENDER-02 | Data tables | pages::render_02_03_tables; tests/live_sites::render_02_read_02_wikipedia |
| RENDER-03 | Narrow tables | pages::render_02_03_tables |
| RENDER-04 | Layout tables | tests/live_sites::render_04_hacker_news |
| RENDER-05 | Lists and quotes | pages::render_05_lists_and_quotes |
| RENDER-06 | Plain-text files | pages::render_06_plain_text |
| RENDER-07 | JSON | pages::render_07_json |
| RENDER-08 | Non-text files | pages::render_08_non_text_files |
| RENDER-11 | Site menus | pages::render_10_11_noscript_and_menus; tests/live_sites::render_11_site_menus |
| RENDER-09 | Full width | pages::render_09_full_width |
| RENDER-10 | No-JavaScript notices | pages::render_10_11_noscript_and_menus |
| CRASH-01 | Stale link selection | browsing::crash_01_stale_link_selection |
| CRASH-02 | Tiny terminal | browsing::crash_02_05_tiny_and_narrow_terminals |
| CRASH-03 | Visual mode on an empty page | browsing::crash_03_copy_mode_on_an_empty_page |
| CRASH-04 | Deeply nested lists | browsing::crash_04_06_deep_nesting |
| CRASH-05 | Extremely narrow terminal | browsing::crash_02_05_tiny_and_narrow_terminals |
| CRASH-06 | Extremely deep nesting | browsing::crash_04_06_deep_nesting |
| NAV-01 | Back after typing a URL | browsing::nav_01_02_back_after_typing_and_esc |
| NAV-02 | Esc cancels editing | browsing::nav_01_02_back_after_typing_and_esc |
| NAV-03 | Redirects update the address bar | tests/live_sites::nav_03_redirects_upgraded |
| NAV-04 | Chapter links jump within the page | browsing::nav_04_05_07_jumps_within_a_page |
| NAV-05 | Back from a chapter | browsing::nav_04_05_07_jumps_within_a_page |
| NAV-06 | Fragment on another page | browsing::nav_06_fragment_on_another_page |
| NAV-07 | Fragment link in a new tab | browsing::nav_04_05_07_jumps_within_a_page |
| NAV-08 | Redirects within a local server | browsing::nav_08_redirects_within_a_local_server |
| NAV-09 | HTTP-only sites are not downgraded | tests/live_sites::nav_09_http_only_sites_arent_downgraded |
| FORM-01 | Search box | forms::form_01_search_box |
| FORM-02 | Search in a table | forms in tables and search fields: forms::form_01_search_box, pages::render_02_03_tables |
| FORM-03 | Checkboxes, options and choices | forms::form_03_checkboxes_radios_and_choices |
| FORM-04 | Passwords | forms::form_04_passwords_show_as_dots |
| FORM-06 | Forms can't reach your local network | forms::form_06_local_pages_may_send_locally |
| FORM-07 | Disguised links | forms::form_07_disguised_links |
| FORM-08 | Passwords in typed addresses | forms::form_08_passwords_in_typed_addresses |
| FORM-05 | Typing keeps up | forms::form_05_typing_keeps_up_on_a_large_page |
| HIST-01 | Back keeps your place | browsing::hist_01_02_back_forward_and_reload_keep_your_place |
| HIST-02 | Forward and reload | browsing::hist_01_02_back_forward_and_reload_keep_your_place |
| HIST-03 | Paging keys | browsing::hist_03_paging_keys |
| HIST-04 | Links to /files/ pages | browsing::hist_04_files_links_that_are_pages |
| SET-01 | Settings template | settings::set_01_settings_template |
| SET-02 | Search engine | settings::set_02_search_engine |
| SET-03 | Home page | settings::set_03_home_page |
| SET-04 | Bad settings | settings::set_04_bad_settings |
| SET-05 | Bookmarks | settings::set_05_bookmarks |
| SET-06 | Bookmarks under the Linux sandbox | CI's Linux job runs the terminal suite inside Landlock (settings::set_05_bookmarks) |
| SET-11 | User-Agent | tests/live_sites::set_11_honest_identity_passes |
| SET-13 | Copy mode | settings::set_13_copy_mode |
| SET-14 | Settings page | settings::set_14_settings_page |
| SET-15 | I2P lock | settings::set_15_network_lock |
| SET-16 | Archived copy | tests/live_sites::set_16_archived_copy |
| SET-07 | Session history | settings::set_07_session_history |
| SET-08 | Saved history | i2p::set_08_i2p_visits_never_reach_history_txt; settings::set_08_09_saved_and_cleared_history |
| SET-09 | Clearing history | settings::set_08_09_saved_and_cleared_history |
| SET-10 | History under the Linux sandbox | CI's Linux job runs the terminal suite inside Landlock (settings::set_08_09_saved_and_cleared_history) |
| SCROLL-02 | Scrolling stops at the end | scrolling::scroll_02_scrolling_stops_at_the_end |
| SCROLL-03 | `j`/`k` continue from the view | scrolling::scroll_03_j_and_k_continue_from_the_view |
| SCROLL-04 | Visual mode follows the cursor | scrolling::scroll_04_05_copy_mode_follows_the_cursor |
| SCROLL-05 | Wheel scrolling in visual mode | scrolling::scroll_04_05_copy_mode_follows_the_cursor |
| SCROLL-06 | Clicking below the page | scrolling::scroll_06_clicking_below_the_page |
| SCROLL-07 | Quitting while moving the mouse | scrolling::scroll_07_quitting_while_the_mouse_moves |
| SCROLL-08 | Search survives a typo | scrolling::scroll_08_search_survives_a_typo |
| SCROLL-09 | Search jumps | scrolling::scroll_09_search_jumps |
| SCROLL-10 | Source view cursor | scrolling::scroll_10_source_view_cursor |
| PERF-01 | Large pages | browsing::perf_01_02_large_pages |
| PERF-02 | Memory use | browsing::perf_01_02_large_pages |
| PERF-03 | Slow pages don't freeze the UI | browsing::perf_03_slow_pages_dont_freeze |
| PERF-04 | Help page after resizing | browsing::smoke_03_resizing_rewraps |
| DL-01 | Speed and filename | downloads::dl_01_02_speed_filename_and_query_strings |
| DL-02 | Query strings | downloads::dl_01_02_speed_filename_and_query_strings |
| DL-03 | Error pages aren't saved | downloads::dl_03_error_pages_arent_saved |
| DL-04 | No silent overwrite | downloads::dl_04_no_silent_overwrite |
| DL-05 | One download per tab | downloads::dl_05_one_download_per_tab |
| DL-06 | Gatekeeper checks downloads (macOS) | downloads::dl_06_plat_03_05_downloads_are_tagged |
| DL-07 | HTTP download links are upgraded | the same https:// upgrade as NAV-09 (tests/live_sites.rs) and tests/integration_test.rs |
| DL-08 | Resuming over I2P | i2p::dl_08_resuming_over_i2p |
| CLIP-02 | No clipboard available (Linux) | linux::clip_02_no_clipboard |
| SEC-01 | Escape codes are neutralised | security::sec_01_escape_codes_are_neutralised |
| SEC-02 | Link destinations are shown | security::sec_02_link_destinations_are_shown |
| SEC-03 | Non-web links are refused | security::sec_03_04_non_web_and_i2p_links |
| SEC-04 | `.i2p` blocked outside I2P mode | security::sec_03_04_non_web_and_i2p_links |
| SEC-05 | Compression bomb rejected | security::sec_05_compression_bomb |
| SEC-06 | Non-UTF-8 pages still render | security::sec_06_non_utf8_pages |
| SEC-07 | Revoked certificates are rejected | tests/live_tls.rs (weekly; macOS runners skip it, run it locally on a Mac) |
| SEC-08 | Names on your own network | localtest.me is refused automatically (tests/dns.rs, live); a real local name needs your network |
| SEC-10 | Look-alike names | security::sec_10_lookalike_names |
| SEC-11 | Cookies encrypted in the keychain | Linux is automated (CI's linux-system job); macOS's Keychain and its prompt need a person |
| PLAT-01 | Linux sandbox status | linux::plat_01_linux_sandbox_status |
| PLAT-02 | Linux sandbox doesn't break browsing | CI's Linux job runs the whole terminal suite inside Landlock |
| PLAT-09 | macOS sandbox status | the terminal suite runs sandboxed on macOS (main::it_starts_sandboxed...) |
| PLAT-10 | macOS sandbox doesn't break browsing | the suite runs sandboxed; a cargo install and a release archive need a person |
| PLAT-03 | Linux download origin | downloads::dl_06_plat_03_05_downloads_are_tagged |
| PLAT-05 | macOS "Where from" | downloads::dl_06_plat_03_05_downloads_are_tagged |
| PLAT-06 | Device-name downloads | downloads::plat_06_device_names |
| PLAT-07 | Command-line options | downloads::plat_07_command_line_options |
| REL-01 | First beta release | done: every release since beta.1 has run the workflow |
| SMALL-01 | Gemini | small_web::small_01_07_gemini_pages |
| SMALL-02 | Gemini input | small_web::small_02_gemini_input |
| SMALL-03 | Gemini certificates | small_web::small_03_gemini_certificates |
| SMALL-04 | Gopher | small_web::small_04_gopher |
| SMALL-05 | Not in I2P mode | small_web::small_05_not_in_i2p_mode |
| SMALL-06 | Feeds | small_web::small_06_feeds |
| SMALL-07 | Bookmarks and history | small_web::small_01_07_gemini_pages |
| SMALL-08 | Micron (NomadNet) pages: lines, sections, colours, alignment, links, tables, fields | `micron` (all), fuzz target `micron` |
| SMALL-09 | NomadNet pages and files through the Reticulum bridge; the I2P lock | `reticulum`; the bridge itself with real Reticulum: `bridges/reticulum/test_bridge.py` (CI's Linux job) |
| UPD-01 | Updating a release build | main::an_update_is_found_downloaded_and_installed_at_the_next_start |
| UPD-02 | Cargo installs | settings::upd_02_cargo_installs |
| UPD-03 | I2P | settings::upd_03_no_checks_locked_to_i2p |
| KEY-01 | Changed keys in a real terminal | settings::key_01_changed_keys |
| KEY-02 | Mistakes in keys.conf | main::a_mistake_in_keys_conf_is_named |
| IMG-03 | Loading settings | pages::img_03_own_images_load_by_themselves |
| CACHE-01 | Back is instant | browsing::cache_01_back_is_instant |
| CACHE-02 | I2P site gone | i2p::cache_02_i2p_site_gone |
| CACHE-03 | Disk cache | pages::cache_03_disk_cache |
| CACHE-04 | Snapshots | pages::cache_04_snapshots |
| READ-02 | Wikipedia | tests/live_sites::render_02_read_02_wikipedia |
| READ-03 | Stays on | browsing::read_03_reader_mode_stays_on |
| MCP-01 | Listing tools | tools::mcp_01_listing_tools |
| MCP-02 | Confirming a call | tools::mcp_02_03_confirming_a_call; tools::mcp_02_a_tool_that_may_change_things_says_so |
| MCP-03 | Back and forward | tools::mcp_02_03_confirming_a_call |
| MCP-04 | Validation | tools::mcp_04_validation |
| MCP-05 | Download prompt | tools::mcp_05_download_box_keeps_its_keys |
| I2P-01 | Browsing in I2P mode | i2p::i2p_01_04_browsing_through_the_proxy_and_back |
| I2P-02 | HTTPS through I2P | i2p::i2p_02_https_goes_through_the_proxy |
| I2P-03 | Jump services, through the router's confirmation | i2p::i2p_03_jump_services |
| I2P-04 | Toggling back | i2p::i2p_01_04_browsing_through_the_proxy_and_back |
| I2P-06 | This computer in I2P mode: typed addresses direct, sites' links refused | i2p::i2p_06_this_computer_is_reached_directly_but_not_from_links |
| I2P-08 | Address helper links (reg.i2p's), confirmed on the router's page | i2p::i2p_08_address_helpers_are_confirmed_on_the_routers_page |
| ROUTER-01 | Starting and stopping with rynx | main::the_router_starts_with_rynx_and_stops_when_it_quits |
| ROUTER-02 | Kept running | main::a_router_kept_running_outlives_rynx |
| ROUTER-03 | Another router | main::a_router_already_running_is_left_alone |
| ROUTER-04 | Linux | CI's linux-system job: the distribution's i2pd, tests/i2pd::the_real_i2pd_starts_reports_and_stops |

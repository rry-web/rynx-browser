# RYNX BROWSER

A terminal CLI browser programmed in Rust, loosely inspired by other CLI browsers such as **Lynx** or **w3m**.

It supports browser tabs, some mouse support for links/scrolling, and absolutely **zero javascript**.

## Installation

To install, set up Rust on your machine and then run:

bash
cargo run

## Key Bindings & Controls

### Navigation (Normal Mode)
| Key | Action |
| :--- | :--- |
| **`Up` / `Down`** | Scroll the page up or down by 1 line. |
| **`Tab`** | Cycle through links visible on the screen. |
| **`Enter`** | Open the currently selected link. |
| **`Backspace`** / **`Left`** | Go back to the previous page in history. |

### Tab Management
| Key | Action |
| :--- | :--- |
| **`n`** | Open a new, blank tab. |
| **`t`** | Open the **currently highlighted link** in a new tab. |
| **`w`** | Close the current tab. |
| **`]`** | Switch to the **Next** tab. |
| **`[`** | Switch to the **Previous** tab. |

### Browser Controls
| Key | Action |
| :--- | :--- |
| **`e`** | Enter **Edit Mode** to type a URL or search query. |
| **`p`** | Toggle **I2P Mode** (Routes traffic via local proxy `127.0.0.1:4444`). |
| **`q`** | Quit the browser. |

### Edit Mode (URL Bar)
_Active after pressing `e`_
| Key | Action |
| :--- | :--- |
| **Typing** | Input URL or search terms. |
| **`Enter`** | Submit request (Defaults to **Marginalia Search** if not a valid URL). |
| **`Esc`** | Cancel editing and return to Normal Mode. |

### Mouse Support
| Action | Function |
| :--- | :--- |
| **Scroll Wheel** | Scroll page up/down by 3 lines. |
| **Left Click** | Open the clicked link. |
| **`Ctrl` + Click** | Open the clicked link in a **New Tab**. |

## Roadmap

This is a hobby project, but I'm interested in:
1. Finishing my i2pd integration implementation.
2. Adding integration support for Model Context Protocol as a workaround to the lack of javascript.
3. Adding clipboard support so you can copy and paste, especially into the address bar.
4. Having some support for displaying images and downloading videos, though I'm not entirely sure in what form factor yet.

If you want to contribute just poke at me on github and I'll get a notification sent to my email.

## License

This project is licensed under GPLv3

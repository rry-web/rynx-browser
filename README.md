# RYNX BROWSER

A terminal CLI browser programmed in Rust, loosely inspired by other CLI browsers such as **Lynx** or **w3m**.

It supports browser tabs, some mouse support for links/scrolling, and absolutely **zero javascript**.

## Installation

To install, set up Rust on your machine and then run:

cargo run

## Key Bindings & Controls

### Navigation (Normal Mode)
| Key | Action |
| :--- | :--- |
| **`h / j / k / l`** | Move cursor (Vim-style). View scrolls to follow. (Few minor issues with horizontal navigation, in progress) |
| **`Up / Down`** | Scroll the page up or down by 1 line. |
| **`Tab / Shift + Tab`** | Cycle through links visible on the screen. (Forward/Backward) |
| **`Enter`** | Open the currently selected link. |
| **`Backspace / Left`** | Go back to the previous page in history. |
| **`d`** | Download the currently selected link. |
| **`Esc`** | Clear finished or failed downloads. |

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
| **`/`** | Enter **Search Mode** to search within the current page. |
| **`p`** | Toggle **I2P Mode** (Routes traffic via local proxy `127.0.0.1:4444`). |
| **`Shift + v`** | Toggle Page Source View. |
| **`q`** | Quit the browser. |

### Visual Mode ###
| Key | Action |
| :--- | :--- |
| **`v`** | Enter visual mode within the browser. |
| **`y`** | Copy text to clipboard. |

### Search Mode (Press '/')
| Key | Action |
| :--- | :--- |
| **Typing** | Input search query with live results. |
| **`>`** | Jump to next search match. |
| **`<`** | Jump to previous search match. |
| **`Enter`** | Finish search and return to Normal Mode. |
| **`Esc`** | Cancel search and return to Normal Mode. |
| **`Backspace`** | Remove characters from search query. |

### Edit Mode (URL Bar)
_Active after pressing `e`_
| Key | Action |
| :--- | :--- |
| **Typing** | Input URL or search terms. |
| **`Enter`** | Submit request (Defaults to **Marginalia Search** if not a valid URL). |
| **`Esc`** | Cancel editing and return to Normal Mode. |
| **`Ctrl + u`** | Clear address bar. |
| **`Ctrl + y`** | Copy address to clipboard. |
| **`Ctrl + v`** | Paste from clipboard. |
| **`Ctrl + k`** | Clear address and paste from clipboard. |


### Mouse Support
| Action | Function |
| :--- | :--- |
| **Scroll Wheel** | Scroll page up/down by 3 lines. |
| **Left Click** | Open the clicked link. |
| **`Ctrl` + Click** | Open the clicked link in a **New Tab**. |

## Roadmap
- [x] **I2P Integration**: Routes via local proxy `127.0.0.1:4444`.
- [x] **Download Manager**: Memory-efficient streaming with progress tracking.
- [x] **Clipboard Support**: Full copy/paste support for URL bar and content.
- [x] **Search Functionality**: Character-level search with highlighting and navigation.
- [ ] **MCP Support**: Integration for Model Context Protocol as a JS alternative.
- [ ] **High-Fidelity Images**: Implementing `ratatui-image` for Sixel/Kitty support.
- [ ] **Text Input**: For entering usernames, passwords, or messages into text boxes.
- [ ] **RSS and Gopher Support**: Additional protocols to add support for.

And many other potential features down the line!

If you want to contribute just poke at me on github and I'll get a notification sent to my email.

## License

This project is licensed under GPLv3

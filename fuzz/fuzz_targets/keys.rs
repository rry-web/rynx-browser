//! A keys.conf file: read, and every pressed key translated without a crash
#![no_main]
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use libfuzzer_sys::fuzz_target;
use rynx::keys::Keymap;

fuzz_target!(|data: &[u8]| {
    let (keymap, _) = Keymap::parse(&String::from_utf8_lossy(data));
    for c in ['q', 'r', 'G', '?', ' '] {
        for modifiers in [KeyModifiers::empty(), KeyModifiers::CONTROL, KeyModifiers::ALT] {
            keymap.translate(KeyEvent::new(KeyCode::Char(c), modifiers));
        }
    }
    keymap.translate(KeyEvent::new(KeyCode::F(5), KeyModifiers::empty()));
    keymap.page_html();
});

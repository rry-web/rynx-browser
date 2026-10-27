//! A settings file: read, written back, and read again unchanged
#![no_main]
use libfuzzer_sys::fuzz_target;
use rynx::settings::Settings;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let (settings, _) = Settings::parse(&text);
    let (again, problems) = Settings::parse(&settings.to_file_text());
    assert!(problems.is_empty(), "the written file has problems: {problems:?}");
    assert_eq!(again, settings);
});

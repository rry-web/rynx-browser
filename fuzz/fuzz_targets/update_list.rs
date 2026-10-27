//! GitHub's release list and a checksum list, as the updater reads them
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let current = semver::Version::parse("0.2.0-beta.6").unwrap();
    if let Some(release) = rynx::update::newest(&text, &current, true) {
        // Only ever something newer, with this platform's build
        assert!(release.version > current);
        assert!(release.program_url.len() > 0);
    }
    rynx::update::checksum_for(&text, "rynx-1.0.0-x86_64-unknown-linux-gnu");
});

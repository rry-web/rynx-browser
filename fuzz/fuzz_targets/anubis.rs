//! A page that may be an Anubis bot check: the challenge read from it, and the answer's address
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let html = String::from_utf8_lossy(data);
    if let Some(challenge) = rynx::anubis::detect(&html) {
        // The answer only ever goes back to the page's own site
        if let Ok(address) =
            rynx::anubis::pass_address("https://site.example/page", &challenge, None, Default::default())
        {
            assert!(address.starts_with("https://site.example/"), "{address}");
        }
    }
});

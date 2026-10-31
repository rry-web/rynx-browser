//! Encrypted DNS and Encrypted Client Hello against real services. They need the internet, so
//! they're ignored by default (CI runs them weekly): `cargo test --test encrypted_dns -- --ignored`.
//! A file of their own, since the dns setting applies to the whole program.

use rynx::network::{NetworkManager, get_following_redirects, read_text_limited};

/// Cloudflare's trace page says whether the site's name arrived encrypted ("sni=encrypted")
const TRACE: &str = "https://crypto.cloudflare.com/cdn-cgi/trace";

async fn trace() -> String {
    let manager = NetworkManager::new().unwrap();
    let response = get_following_redirects(manager.get_client(false), TRACE, false).await.unwrap();
    read_text_limited(response, 64 * 1024).await.unwrap()
}

#[tokio::test]
#[ignore]
async fn names_are_looked_up_encrypted_and_ech_hides_the_site_name() {
    // The system's resolver, and no ECH: the name is sent as usual
    rynx::dns::use_dns("system");
    assert!(rynx::ech::key_for("crypto.cloudflare.com").await.is_none(), "no ECH without encrypted DNS");
    let plain = trace().await;
    assert!(plain.contains("sni=plaintext"), "{plain}");

    for provider in ["quad9", "mullvad", "cloudflare"] {
        rynx::dns::use_dns(provider);
        let addresses = rynx::dns::resolve("example.com", 443).await.unwrap_or_else(|e| panic!("{provider}: {e}"));
        assert!(!addresses.is_empty(), "{provider}");
        // A public name pointing at this computer is still refused
        assert!(rynx::dns::resolve("localtest.me", 80).await.is_err(), "{provider}");
    }

    rynx::dns::use_dns("quad9");
    assert!(rynx::ech::key_for("crypto.cloudflare.com").await.is_some(), "Cloudflare publishes an ECH key");
    let encrypted = trace().await;
    assert!(encrypted.contains("sni=encrypted"), "{encrypted}");
}

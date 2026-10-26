//! Live TLS checks against public test servers. They need internet access, so they're ignored
//! by default; run them with `cargo test --test live_tls -- --ignored`.

use rynx_browser::network::NetworkManager;

async fn fetch(url: &str) -> Result<reqwest::Response, String> {
    let manager = NetworkManager::new().unwrap();
    manager
        .get_client(false)
        .get(url)
        .send()
        .await
        .map_err(|e| {
            // The useful detail is at the bottom of the error chain
            let mut detail = e.to_string();
            let mut source = std::error::Error::source(&e);
            while let Some(s) = source {
                detail = s.to_string();
                source = s.source();
            }
            detail
        })
}

#[tokio::test]
#[ignore]
async fn valid_site_loads_over_http2() {
    let resp = fetch("https://www.rust-lang.org/").await.expect("valid site failed");
    assert_eq!(resp.version(), reqwest::Version::HTTP_2);
}

#[tokio::test]
#[ignore]
async fn bad_certificates_are_rejected() {
    for url in [
        "https://expired.badssl.com/",
        "https://self-signed.badssl.com/",
        "https://wrong.host.badssl.com/",
        "https://untrusted-root.badssl.com/",
    ] {
        assert!(fetch(url).await.is_err(), "{url} was accepted");
    }
}

#[tokio::test]
#[ignore]
async fn old_tls_versions_are_rejected() {
    assert!(fetch("https://tls-v1-0.badssl.com:1010/").await.is_err());
    assert!(fetch("https://tls-v1-1.badssl.com:1011/").await.is_err());
    assert!(fetch("https://tls-v1-2.badssl.com:1012/").await.is_ok());
}

/// Revocation is checked by the OS verifier on macOS and Windows; Linux has no system revocation
/// checking, so there this cert is still accepted
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[tokio::test]
#[ignore]
async fn revoked_certificate_is_rejected() {
    let err = fetch("https://revoked.badssl.com/").await.expect_err("revoked cert accepted");
    assert!(err.contains("Revoked"), "{err}");
}

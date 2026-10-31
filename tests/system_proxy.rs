//! A proxy from the system (here HTTP_PROXY) looks names up itself, past rynx's check for public
//! names pointing at the local network and past encrypted DNS, so it's used only when the
//! system_proxy setting asks. A file of its own: the environment is the whole program's.

use rynx::network::{NetworkManager, get_following_redirects};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn the_systems_proxy_is_used_only_when_asked() {
    // A stand-in proxy, answering whatever it's asked for
    let proxy = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(200).set_body_string("from the proxy"))
        .mount(&proxy)
        .await;
    // SAFETY: no other thread of this test program reads the environment meanwhile
    unsafe {
        std::env::set_var("HTTP_PROXY", proxy.uri());
        std::env::set_var("http_proxy", proxy.uri());
        // Addresses are tried as https:// first
        std::env::set_var("HTTPS_PROXY", proxy.uri());
        std::env::set_var("https_proxy", proxy.uri());
        std::env::remove_var("NO_PROXY");
        std::env::remove_var("no_proxy");
    }
    // Looked up by the system, so the name simply doesn't exist without the proxy
    rynx::dns::use_dns("system");
    let address = "http://rynx-proxy-check.invalid/page";

    let manager = NetworkManager::new().unwrap();
    let direct = get_following_redirects(manager.get_client(false), address, false).await;
    assert!(direct.is_err(), "the name was looked up by rynx, not the proxy");
    assert!(proxy.received_requests().await.unwrap().is_empty(), "the proxy was used without asking");

    rynx::network::use_system_proxy(true);
    let manager = NetworkManager::new().unwrap();
    // The stand-in can't carry https:// (CONNECT) on, but it's what was asked
    let _ = get_following_redirects(manager.get_client(false), address, false).await;
    let asked = proxy.received_requests().await.unwrap();
    assert!(!asked.is_empty(), "the proxy wasn't used when asked");
    rynx::network::use_system_proxy(false);
}

#[test]
fn the_setting_is_checked_and_saved() {
    use rynx::settings::Settings;
    assert!(!Settings::default().system_proxy);
    let (settings, problems) = Settings::parse("system_proxy = on");
    assert!(problems.is_empty() && settings.system_proxy);
    assert!(Settings::parse(&settings.to_file_text()).0.system_proxy);
    assert_eq!(Settings::parse("system_proxy = maybe").1.len(), 1);
}

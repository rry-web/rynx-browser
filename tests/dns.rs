use rynx::dns::{allow_local_names, check, is_local_address, is_local_name, resolve};
use std::net::{IpAddr, SocketAddr};

fn addresses(ips: &[&str]) -> Vec<SocketAddr> {
    ips.iter().map(|ip| SocketAddr::new(ip.parse::<IpAddr>().unwrap(), 443)).collect()
}

#[test]
fn names_local_by_their_form() {
    for name in [
        "localhost",
        "printer.local",
        "router.lan",
        "nas.home.arpa",
        "db.internal",
        "box",
        "Printer.Local.",
        "x.localhost",
    ] {
        assert!(is_local_name(name), "{name}");
    }
    for name in ["example.com", "local.example.com", "lan.example", "localhost.example.com", "stats.i2p"] {
        assert!(!is_local_name(name), "{name}");
    }
}

#[test]
fn local_addresses() {
    for ip in [
        "127.0.0.1",
        "10.1.2.3",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.1.1",
        "0.0.0.0",
        "100.64.0.1",
        "::1",
        "fd00::1",
        "fe80::1",
        "::ffff:192.168.1.1",
    ] {
        assert!(is_local_address(ip.parse().unwrap()), "{ip}");
    }
    for ip in ["8.8.8.8", "100.128.0.1", "172.32.0.1", "2606:4700::1111", "::ffff:8.8.8.8"] {
        assert!(!is_local_address(ip.parse().unwrap()), "{ip}");
    }
}

/// One test, since the allowed names are shared by the whole program
#[test]
fn a_public_name_pointing_at_the_local_network_is_refused_unless_allowed() {
    allow_local_names(&[]);
    assert!(check("example.com", &addresses(&["93.184.216.34"])).is_ok());
    let refused = check("evil.example", &addresses(&["192.168.1.1"])).unwrap_err();
    assert!(refused.contains("evil.example points at 192.168.1.1"), "{refused}");
    assert!(refused.contains("local_names"), "the way out is named: {refused}");
    // One local address among public ones is enough to refuse: the connection could use it
    assert!(check("evil.example", &addresses(&["93.184.216.34", "127.0.0.1"])).is_err());
    assert!(check("evil.example", &addresses(&["::1"])).is_err());
    // Local by their form: looked up as usual
    assert!(check("printer.local", &addresses(&["192.168.1.50"])).is_ok());
    assert!(check("nas", &addresses(&["10.0.0.2"])).is_ok());

    allow_local_names(&["Intranet.Example.com".to_string(), " wiki.example.org. ".to_string()]);
    assert!(check("intranet.example.com", &addresses(&["10.0.0.5"])).is_ok());
    assert!(check("docs.intranet.example.com", &addresses(&["10.0.0.6"])).is_ok());
    assert!(check("wiki.example.org", &addresses(&["10.0.0.7"])).is_ok());
    // Only those names and their subdomains
    assert!(check("evilintranet.example.com", &addresses(&["10.0.0.5"])).is_err());
    assert!(check("example.com", &addresses(&["10.0.0.5"])).is_err());
    allow_local_names(&[]);
    assert!(check("intranet.example.com", &addresses(&["10.0.0.5"])).is_err());
}

#[tokio::test]
async fn addresses_and_local_names_are_looked_up_as_usual() {
    assert_eq!(
        resolve("127.0.0.1", 80).await.unwrap(),
        addresses(&["127.0.0.1"]).iter().map(|a| SocketAddr::new(a.ip(), 80)).collect::<Vec<_>>()
    );
    assert_eq!(resolve("[::1]", 80).await.unwrap()[0].ip(), "::1".parse::<IpAddr>().unwrap());
    assert!(resolve("localhost", 80).await.unwrap().iter().all(|a| a.ip().is_loopback()));
}

#[test]
fn public_pages_cant_send_the_browser_to_local_names() {
    let public = url::Url::parse("https://example.com/").unwrap();
    for to in ["http://printer.local/", "http://router.lan/admin", "http://nas/", "https://192.168.1.1/"] {
        let to = url::Url::parse(to).unwrap();
        assert!(rynx::network::check_redirect(&public, &to).is_err(), "{to}");
    }
    // .i2p sites aren't local ones
    assert!(!rynx::network::is_local_network_url("http://stats.i2p/"));
    // A local page may link within the local network
    let local = url::Url::parse("http://router.lan/").unwrap();
    assert!(rynx::network::check_redirect(&local, &url::Url::parse("http://router.lan/login").unwrap()).is_ok());
}

#[test]
fn local_names_setting() {
    let (settings, problems) = rynx::settings::Settings::parse("local_names = Intranet.example.com, wiki.example.org");
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(settings.local_names, vec!["intranet.example.com", "wiki.example.org"]);
    let (again, _) = rynx::settings::Settings::parse(&settings.to_file_text());
    assert_eq!(again.local_names, settings.local_names);
    let (_, problems) = rynx::settings::Settings::parse("local_names = not a name");
    assert_eq!(problems.len(), 1, "{problems:?}");
}

/// DNS rebinding for real: localtest.me is a public name whose DNS answers 127.0.0.1 (and ::1). The
/// browser's client must refuse it, though the server is there, unless the name is allowed.
/// Needs the internet: `cargo test --test dns -- --ignored`
#[tokio::test]
#[ignore]
async fn a_public_name_answering_127_0_0_1_is_refused() {
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::any};
    let server = MockServer::start().await;
    Mock::given(any()).respond_with(ResponseTemplate::new(200).set_body_string("local secret")).mount(&server).await;
    let port = server.address().port();
    let manager = rynx::network::NetworkManager::new().unwrap();
    let url = format!("http://localtest.me:{}/", port);

    allow_local_names(&[]);
    let error = rynx::network::get_following_redirects(manager.get_client(false), &url, false).await.unwrap_err();
    println!("{error}");
    assert!(error.contains("localtest.me points at"), "{error}");
    assert!(server.received_requests().await.unwrap().is_empty(), "the local server was reached");

    allow_local_names(&["localtest.me".to_string()]);
    // Allowed, the connection is made (over https, which the mock doesn't speak, so it fails later)
    let allowed = rynx::network::get_following_redirects(manager.get_client(false), &url, false).await;
    assert!(!allowed.as_ref().err().is_some_and(|e| e.contains("points at")), "{allowed:?}");
    allow_local_names(&[]);
}

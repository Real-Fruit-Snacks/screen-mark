//! Which proxy an update fetch goes through (`update::proxy::choose`).

use screen_mark::update::proxy::{choose, shown, SystemProxy};

const GITHUB: &str =
    "https://github.com/Real-Fruit-Snacks/screen-mark/releases/latest/download/latest.json";

fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |name| {
        vars.iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.to_string())
    }
}

fn windows(server: &str, bypass: &str) -> SystemProxy {
    SystemProxy {
        server: server.into(),
        bypass: bypass.into(),
    }
}

#[test]
fn no_proxy_anywhere_connects_directly() {
    assert_eq!(choose(GITHUB, "", &env(&[]), None), None);
}

#[test]
fn the_setting_wins_over_everything() {
    let vars = [("HTTPS_PROXY", "http://env:1")];
    let system = windows("system:2", "");
    assert_eq!(
        choose(GITHUB, "http://setting:3", &env(&vars), Some(&system)),
        Some("http://setting:3".into())
    );
    // Without a scheme it is an HTTP proxy.
    assert_eq!(
        choose(GITHUB, " proxy.corp:8080 ", &env(&[]), None),
        Some("http://proxy.corp:8080".into())
    );
}

#[test]
fn none_in_the_setting_means_direct_even_with_a_proxy_around() {
    let vars = [("HTTPS_PROXY", "http://env:1")];
    let system = windows("system:2", "");
    for word in ["none", "NONE", "direct"] {
        assert_eq!(
            choose(GITHUB, word, &env(&vars), Some(&system)),
            None,
            "{word}"
        );
    }
}

#[test]
fn the_environment_proxy_is_picked_by_scheme() {
    let vars = [
        ("HTTPS_PROXY", "http://secure:1"),
        ("HTTP_PROXY", "http://plain:2"),
    ];
    assert_eq!(
        choose(GITHUB, "", &env(&vars), None),
        Some("http://secure:1".into())
    );
    assert_eq!(
        choose(
            "http://intranet/downloads/latest.json",
            "",
            &env(&vars),
            None
        ),
        Some("http://plain:2".into())
    );
    // Lower case works, and ALL_PROXY is the fallback for both.
    let lower = [("https_proxy", "lower:3")];
    assert_eq!(
        choose(GITHUB, "", &env(&lower), None),
        Some("http://lower:3".into())
    );
    let all = [("ALL_PROXY", "http://all:4")];
    assert_eq!(
        choose("http://intranet/x", "", &env(&all), None),
        Some("http://all:4".into())
    );
    // An empty variable is not a proxy.
    let empty = [("HTTPS_PROXY", " "), ("ALL_PROXY", "http://all:4")];
    assert_eq!(
        choose(GITHUB, "", &env(&empty), None),
        Some("http://all:4".into())
    );
}

#[test]
fn no_proxy_sends_matching_hosts_direct() {
    let with = |skip: &'static str| {
        let vars = [("HTTPS_PROXY", "http://env:1"), ("NO_PROXY", skip)];
        let lookup = env(&vars);
        choose(GITHUB, "", &lookup, None)
    };
    assert_eq!(with("github.com"), None);
    assert_eq!(with(".github.com"), None);
    assert_eq!(with("*.github.com"), None);
    assert_eq!(with("localhost, GitHub.com:443"), None);
    assert_eq!(with("*"), None);
    // Only whole names: "hub.com" is not github.com.
    assert_eq!(with("hub.com"), Some("http://env:1".into()));
    assert_eq!(with("example.org"), Some("http://env:1".into()));
    // A subdomain of a listed name goes direct.
    let vars = [
        ("HTTPS_PROXY", "http://env:1"),
        ("no_proxy", "corp.example"),
    ];
    assert_eq!(
        choose(
            "https://files.corp.example/latest.json",
            "",
            &env(&vars),
            None
        ),
        None
    );
}

#[test]
fn the_windows_proxy_is_used_when_nothing_else_names_one() {
    let system = windows("system.corp:8080", "");
    assert_eq!(
        choose(GITHUB, "", &env(&[]), Some(&system)),
        Some("http://system.corp:8080".into())
    );
    // The environment comes first.
    let vars = [("HTTPS_PROXY", "http://env:1")];
    assert_eq!(
        choose(GITHUB, "", &env(&vars), Some(&system)),
        Some("http://env:1".into())
    );
}

#[test]
fn a_windows_proxy_per_protocol_is_picked_by_scheme() {
    let system = windows("http=plain:80;https=secure:443;socks=sock:1080", "");
    assert_eq!(
        choose(GITHUB, "", &env(&[]), Some(&system)),
        Some("http://secure:443".into())
    );
    assert_eq!(
        choose("http://intranet/latest.json", "", &env(&[]), Some(&system)),
        Some("http://plain:80".into())
    );
    // Only a SOCKS proxy: updates go direct.
    let socks = windows("socks=sock:1080", "");
    assert_eq!(choose(GITHUB, "", &env(&[]), Some(&socks)), None);
}

#[test]
fn windows_exceptions_send_matching_hosts_direct() {
    let check = |bypass: &str, url: &str| {
        let system = windows("proxy:8080", bypass);
        choose(url, "", &env(&[]), Some(&system))
    };
    assert_eq!(check("*.github.com;github.com", GITHUB), None);
    assert_eq!(check("GITHUB.COM", GITHUB), None);
    assert_eq!(check("git*.com", GITHUB), None);
    assert_eq!(check("10.*", "http://10.1.2.3/latest.json"), None);
    // <local>: a name without a dot, such as an intranet server.
    assert_eq!(check("<local>", "http://fileserver/latest.json"), None);
    assert_eq!(check("<local>", GITHUB), Some("http://proxy:8080".into()));
    // A pattern has to match the whole name.
    assert_eq!(check("github.co", GITHUB), Some("http://proxy:8080".into()));
    assert_eq!(
        check("*.github.com", GITHUB),
        Some("http://proxy:8080".into())
    );
}

#[test]
fn hosts_are_read_past_ports_users_and_paths() {
    let vars = [("HTTPS_PROXY", "http://env:1"), ("NO_PROXY", "mirror.corp")];
    for url in [
        "https://mirror.corp:8443/x/latest.json",
        "https://user@mirror.corp/latest.json",
        "https://MIRROR.CORP?x",
    ] {
        assert_eq!(choose(url, "", &env(&vars), None), None, "{url}");
    }
    let v6 = [("HTTPS_PROXY", "http://env:1"), ("NO_PROXY", "[::1]")];
    assert_eq!(
        choose("https://[::1]:8443/latest.json", "", &env(&v6), None),
        None
    );
}

#[test]
fn a_password_in_a_proxy_is_never_shown() {
    assert_eq!(
        shown("http://me:secret@proxy:8080"),
        "http://me:***@proxy:8080"
    );
    assert_eq!(shown("me:secret@proxy:8080"), "me:***@proxy:8080");
    assert_eq!(shown("http://proxy:8080"), "http://proxy:8080");
}

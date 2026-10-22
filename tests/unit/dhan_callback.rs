use super::*;

fn redirect() -> Url {
    parse_redirect("https://login.example.in/dhan/callback").unwrap()
}

#[test]
fn a_redirect_needs_a_scheme_host_and_path_of_its_own() {
    assert!(parse_redirect("https://login.example.in/dhan/callback").is_ok());
    assert!(parse_redirect("http://127.0.0.1:47440/dhan/callback").is_ok());
    assert!(parse_redirect("ftp://x/dhan").is_err());
    assert!(parse_redirect("https://login.example.in/").is_err());
    assert!(parse_redirect("https://u:p@login.example.in/cb").is_err());
    assert!(parse_redirect("https://login.example.in/cb#frag").is_err());
    assert!(parse_redirect("https://login.example.in/cb?tokenId=x").is_err());
    assert!(parse_redirect("https://login.example.in/{cb}").is_err());
    assert!(parse_redirect("not a url").is_err());
}

#[test]
fn the_route_is_the_path_without_a_trailing_slash() {
    assert_eq!(
        route_of(&parse_redirect("https://x.in/dhan/callback/").unwrap()),
        "/dhan/callback"
    );
}

#[test]
fn exactly_one_valid_token_id_is_taken_from_the_callback() {
    let url = redirect();
    assert_eq!(callback_token("tokenId=abc123", &url).unwrap(), "abc123");
    assert!(callback_token("", &url).is_err());
    assert!(callback_token("tokenId=", &url).is_err());
    assert!(callback_token("tokenId=a&tokenId=b", &url).is_err());
    assert!(
        callback_token("tokenId=a%20b", &url).is_err(),
        "a token with whitespace is refused"
    );
    assert!(
        callback_token("tokenId=a&extra=1", &url).is_err(),
        "an extra parameter is refused"
    );
    assert!(callback_token(&format!("tokenId={}", "a".repeat(9000)), &url).is_err());
}

#[test]
fn a_configured_query_must_come_back_unchanged() {
    let url = parse_redirect("https://x.in/cb?account=main").unwrap();
    assert_eq!(callback_token("account=main&tokenId=t", &url).unwrap(), "t");
    assert!(callback_token("account=other&tokenId=t", &url).is_err());
    assert!(callback_token("tokenId=t", &url).is_err());
}

#[tokio::test]
async fn the_gate_accepts_only_while_a_login_waits_and_closes_when_dropped() {
    let gate = CallbackGate::new(redirect());
    assert_eq!(gate.deliver("tokenId=early"), Delivery::NoLoginPending);
    assert_eq!(gate.waiting_for(), None);

    let mut ticket = gate.open("main");
    assert_eq!(gate.waiting_for().as_deref(), Some("main"));
    assert_eq!(
        gate.deliver("garbage"),
        Delivery::Invalid,
        "a malformed callback does not end the login"
    );
    assert_eq!(gate.deliver("tokenId=good"), Delivery::Accepted);
    assert_eq!(ticket.candidates.recv().await.as_deref(), Some("good"));

    drop(ticket);
    assert_eq!(gate.deliver("tokenId=late"), Delivery::NoLoginPending);
    assert_eq!(
        gate.redirect().as_str(),
        "https://login.example.in/dhan/callback"
    );
}

#[test]
fn the_gate_caps_attempts_so_the_public_route_is_not_a_proxy() {
    let gate = CallbackGate::new(redirect());
    let _ticket = gate.open("main");
    for _ in 0..20 {
        assert_eq!(gate.deliver("bad"), Delivery::Invalid);
    }
    assert_eq!(gate.deliver("tokenId=good"), Delivery::TooManyAttempts);
}

use super::*;
use crate::test_support::DHAN_SEED;

fn form() -> AccountForm {
    AccountForm {
        alias: "  Main ".into(),
        client_id: " 1100000001 ".into(),
        api_key: "key-1".into(),
        api_secret: "secret-1".into(),
        account_id: "9999999999".into(),
        pin: "123456".into(),
        dhan_totp_secret: "jbsw y3dp ehpk 3pxp".into(),
    }
}

fn fields(problems: &[FieldProblem]) -> Vec<&'static str> {
    problems.iter().map(|problem| problem.field).collect()
}

#[test]
fn a_valid_form_is_trimmed_lowercased_and_its_seed_decoded() {
    let account = parse(&form()).expect("a valid form parses");
    assert_eq!(account.alias, "main");
    assert_eq!(account.client_id, "1100000001");
    assert_eq!(account.pin, "123456");
    assert_eq!(
        account.dhan_totp_seed,
        crate::otp::decode_base32(DHAN_SEED).unwrap()
    );
}

#[test]
fn every_bad_field_is_named_and_no_value_is_echoed() {
    let bad = AccountForm {
        alias: "ma-in!".into(),
        client_id: "".into(),
        api_key: "has space".into(),
        api_secret: "".into(),
        account_id: "x".repeat(MAX_FIELD + 1),
        pin: "12ab".into(),
        dhan_totp_secret: "not base32!".into(),
    };
    let problems = parse(&bad).expect_err("nothing here is valid");
    assert_eq!(
        fields(&problems),
        vec![
            "alias",
            "client_id",
            "api_key",
            "api_secret",
            "account_id",
            "pin",
            "dhan_totp_secret"
        ]
    );
    let text = serde_json::to_string(&problems).unwrap();
    for value in ["ma-in!", "has space", "12ab", "not base32!"] {
        assert!(!text.contains(value), "a problem must not repeat {value:?}");
    }
}

#[test]
fn aliases_that_collide_with_routes_or_are_too_long_are_refused() {
    for alias in ["login", "LOGIN", &"a".repeat(ALIAS_MAX + 1)] {
        let problems = parse(&AccountForm {
            alias: alias.to_owned(),
            ..form()
        })
        .expect_err("refused");
        assert_eq!(fields(&problems), vec!["alias"], "alias {alias:?}");
    }
}

#[test]
fn a_dhan_seed_must_carry_at_least_80_bits_and_a_pin_only_digits() {
    let short = parse(&AccountForm {
        dhan_totp_secret: "JBSWY3DP".into(),
        ..form()
    })
    .expect_err("40 bits is too short");
    assert_eq!(fields(&short), vec!["dhan_totp_secret"]);

    for pin in ["123", "123456789", "12 456"] {
        let problems = parse(&AccountForm {
            pin: pin.into(),
            ..form()
        })
        .expect_err("refused");
        assert_eq!(fields(&problems), vec!["pin"], "pin {pin:?}");
    }
}

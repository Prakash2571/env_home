use super::*;
use crate::otp::code_at;

const SECRET: &[u8] = b"12345678901234567890";
const NOW: u64 = 1_800_000_000;
const ENGINE: &str = "198.51.100.10";
const ATTACKER: &str = "203.0.113.66";

fn attempt<'a>(client: &'a str, source: &'a str, code: &'a str, now: u64) -> Attempt<'a> {
    Attempt {
        client_id: client,
        source,
        secret: Some(SECRET),
        code,
        now,
    }
}

fn unknown<'a>(client: &'a str, source: &'a str, code: &'a str, now: u64) -> Attempt<'a> {
    Attempt {
        secret: None,
        ..attempt(client, source, code, now)
    }
}

#[test]
fn a_current_code_passes_once_and_a_replay_is_refused_from_any_source() {
    let guard = TokenGuard::default();
    let code = code_at(SECRET, NOW);
    assert_eq!(
        guard.check_code(&attempt("1", ENGINE, &code, NOW)),
        Ok(step_of(NOW))
    );
    assert_eq!(
        guard.check_code(&attempt("1", ATTACKER, &code, NOW + 1)),
        Err(Refusal::Reused)
    );
    let next = code_at(SECRET, NOW + 30);
    assert!(
        guard
            .check_code(&attempt("1", ENGINE, &next, NOW + 30))
            .is_ok()
    );
}

#[test]
fn one_step_of_clock_drift_is_tolerated_and_two_are_not() {
    let check = |code_time: u64| {
        TokenGuard::default().check_code(&attempt("1", ENGINE, &code_at(SECRET, code_time), NOW))
    };
    assert!(check(NOW - 30).is_ok());
    assert!(check(NOW + 30).is_ok());
    assert!(matches!(check(NOW - 60), Err(Refusal::Wrong { .. })));
}

#[test]
fn an_older_code_after_a_newer_one_is_a_replay() {
    let guard = TokenGuard::default();
    assert!(
        guard
            .check_code(&attempt("1", ENGINE, &code_at(SECRET, NOW + 30), NOW))
            .is_ok()
    );
    assert_eq!(
        guard.check_code(&attempt("1", ENGINE, &code_at(SECRET, NOW), NOW)),
        Err(Refusal::Reused)
    );
}

#[test]
fn five_wrong_codes_lock_that_source_out_for_ten_minutes() {
    let guard = TokenGuard::default();
    for second in 0..4 {
        assert_eq!(
            guard.check_code(&attempt("1", ATTACKER, "000000", NOW + second)),
            Err(Refusal::Wrong { locked_until: None })
        );
    }
    let until = NOW + 4 + LOCKOUT_SECONDS;
    assert_eq!(
        guard.check_code(&attempt("1", ATTACKER, "000000", NOW + 4)),
        Err(Refusal::Wrong {
            locked_until: Some(until)
        })
    );
    let right = code_at(SECRET, NOW + 5);
    assert_eq!(
        guard.check_code(&attempt("1", ATTACKER, &right, NOW + 5)),
        Err(Refusal::Locked { until })
    );
    assert!(
        guard
            .check_code(&attempt("1", ATTACKER, &code_at(SECRET, until), until))
            .is_ok(),
        "the lock lifts on its own"
    );
}

#[test]
fn someone_who_knows_a_client_id_cannot_lock_the_engine_out() {
    let guard = TokenGuard::default();
    for second in 0..20 {
        let _ = guard.check_code(&attempt("1", ATTACKER, "000000", NOW + second));
    }
    assert!(
        guard
            .check_code(&attempt("1", ENGINE, &code_at(SECRET, NOW + 30), NOW + 30))
            .is_ok(),
        "a public client ID must not be enough to deny the trading engine its token"
    );
}

#[test]
fn guessing_from_many_sources_still_trips_the_per_client_budget() {
    let guard = TokenGuard::default();
    let mut locked = None;
    for index in 0..CLIENT_FAILURE_LIMIT {
        let source = format!("203.0.113.{index}");
        if let Err(Refusal::Wrong { locked_until }) =
            guard.check_code(&attempt("1", &source, "000000", NOW))
        {
            locked = locked_until;
        }
    }
    assert!(
        locked.is_some(),
        "30 wrong codes in 10 minutes lock the client everywhere"
    );
    assert!(matches!(
        guard.check_code(&attempt("1", ENGINE, &code_at(SECRET, NOW), NOW)),
        Err(Refusal::Locked { .. })
    ));
}

#[test]
fn an_unknown_client_id_can_never_pass_or_reset_its_counter() {
    let guard = TokenGuard::default();
    for second in 0..4 {
        assert_eq!(
            guard.check_code(&unknown("9", ATTACKER, "123456", NOW + second)),
            Err(Refusal::Wrong { locked_until: None })
        );
    }
    let decoy_code = code_at(&guard.decoy, NOW + 4);
    assert!(matches!(
        guard.check_code(&unknown("9", ATTACKER, &decoy_code, NOW + 4)),
        Err(Refusal::Wrong {
            locked_until: Some(_)
        })
    ));
    assert!(
        matches!(
            guard.check_code(&unknown("9", ATTACKER, "123456", NOW + 5)),
            Err(Refusal::Locked { .. })
        ),
        "the lockout sequence for an unknown ID matches a known one, so it reveals nothing"
    );
}

#[test]
fn the_decoy_seed_differs_per_process() {
    assert_ne!(TokenGuard::default().decoy, TokenGuard::default().decoy);
}

#[test]
fn a_malformed_code_is_refused_without_counting_as_a_guess() {
    let guard = TokenGuard::default();
    for _ in 0..10 {
        assert_eq!(
            guard.check_code(&attempt("1", ATTACKER, "12ab56", NOW)),
            Err(Refusal::Malformed)
        );
    }
    assert!(
        guard
            .check_code(&attempt("1", ATTACKER, &code_at(SECRET, NOW), NOW))
            .is_ok()
    );
}

#[test]
fn each_ip_gets_thirty_requests_a_minute_and_ipv6_is_grouped_by_64() {
    let guard = TokenGuard::default();
    for second in 0..30 {
        assert!(guard.admit_ip(ATTACKER, NOW + second / 10));
    }
    assert!(!guard.admit_ip(ATTACKER, NOW + 5));
    assert!(guard.admit_ip(ENGINE, NOW + 5));
    assert!(guard.admit_ip(ATTACKER, NOW + 61), "the window slides");

    for host in 0..30 {
        assert!(guard.admit_ip(&format!("2001:db8:1:2::{host:x}"), NOW));
    }
    assert!(
        !guard.admit_ip("2001:db8:1:2:ffff::1", NOW),
        "rotating addresses inside one /64 is one source"
    );
    assert_eq!(source_key("2001:db8:1:2:3:4:5:6"), "2001:db8:1:2::/64");
    assert_eq!(source_key("not an ip"), "not an ip");
}

#[test]
fn lockout_alerts_are_throttled_per_client_and_overall() {
    let guard = TokenGuard::default();
    assert!(guard.should_alert("1", NOW));
    assert!(
        !guard.should_alert("1", NOW + 60),
        "one alert per client an hour"
    );
    for index in 0..ALERTS_PER_HOUR - 1 {
        assert!(guard.should_alert(&format!("other{index}"), NOW + 1));
    }
    assert!(
        !guard.should_alert("one-too-many", NOW + 2),
        "and at most ten an hour overall"
    );
    assert!(guard.should_alert("1", NOW + ALERT_GAP_SECONDS + 5));
}

#[test]
fn every_tracked_map_is_bounded_and_fails_closed_when_full() {
    let guard = TokenGuard::default();
    for index in 0..MAX_TRACKED {
        let client = format!("c{index}");
        let _ = guard.check_code(&unknown(&client, ATTACKER, "000000", NOW));
    }
    assert!(matches!(
        guard.check_code(&unknown("one-more", ATTACKER, "000000", NOW)),
        Err(Refusal::Locked { .. })
    ));
    assert!(
        guard
            .check_code(&unknown(
                "one-more",
                ATTACKER,
                "000000",
                NOW + FAILURE_WINDOW_SECONDS + 1
            ))
            .is_err(),
        "idle entries are evicted once they age out"
    );
}

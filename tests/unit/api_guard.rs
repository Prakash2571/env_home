use super::*;
use crate::otp::code_at;

const SECRET: &[u8] = b"12345678901234567890";
const NOW: u64 = 1_800_000_000;

#[test]
fn a_current_code_passes_once_and_a_replay_is_refused() {
    let mut guard = CodeGuard::default();
    let code = code_at(SECRET, NOW);
    assert_eq!(guard.check(SECRET, &code, NOW), Ok(step_of(NOW)));
    assert_eq!(guard.check(SECRET, &code, NOW + 1), Err(Refusal::Reused));
    let next = code_at(SECRET, NOW + 30);
    assert!(guard.check(SECRET, &next, NOW + 30).is_ok());
}

#[test]
fn one_step_of_clock_drift_is_tolerated_and_two_are_not() {
    let mut guard = CodeGuard::default();
    assert!(guard.check(SECRET, &code_at(SECRET, NOW - 30), NOW).is_ok());
    let mut guard = CodeGuard::default();
    assert!(guard.check(SECRET, &code_at(SECRET, NOW + 30), NOW).is_ok());
    let mut guard = CodeGuard::default();
    assert!(matches!(
        guard.check(SECRET, &code_at(SECRET, NOW - 60), NOW),
        Err(Refusal::Wrong { .. })
    ));
}

#[test]
fn an_older_code_after_a_newer_one_is_a_replay() {
    let mut guard = CodeGuard::default();
    assert!(guard.check(SECRET, &code_at(SECRET, NOW + 30), NOW).is_ok());
    assert_eq!(
        guard.check(SECRET, &code_at(SECRET, NOW), NOW),
        Err(Refusal::Reused)
    );
}

#[test]
fn five_wrong_codes_lock_the_client_out_for_ten_minutes() {
    let mut guard = CodeGuard::default();
    for attempt in 0..4 {
        assert_eq!(
            guard.check(SECRET, "000000", NOW + attempt),
            Err(Refusal::Wrong { locked_until: None })
        );
    }
    assert_eq!(
        guard.check(SECRET, "000000", NOW + 4),
        Err(Refusal::Wrong {
            locked_until: Some(NOW + 4 + LOCKOUT_SECONDS)
        })
    );
    let right = code_at(SECRET, NOW + 5);
    assert_eq!(
        guard.check(SECRET, &right, NOW + 5),
        Err(Refusal::Locked {
            until: NOW + 4 + LOCKOUT_SECONDS
        })
    );
    let later = NOW + 4 + LOCKOUT_SECONDS;
    assert!(
        guard.check(SECRET, &code_at(SECRET, later), later).is_ok(),
        "the lock lifts on its own"
    );
}

#[test]
fn failures_older_than_the_window_are_forgotten() {
    let mut guard = CodeGuard::default();
    for attempt in 0..4 {
        let _ = guard.check(SECRET, "000000", NOW + attempt);
    }
    let later = NOW + FAILURE_WINDOW_SECONDS + 10;
    assert_eq!(
        guard.check(SECRET, "000000", later),
        Err(Refusal::Wrong { locked_until: None })
    );
}

#[test]
fn a_malformed_code_is_refused_without_counting_as_a_guess() {
    let mut guard = CodeGuard::default();
    for _ in 0..10 {
        assert_eq!(guard.check(SECRET, "12ab56", NOW), Err(Refusal::Malformed));
    }
    assert!(guard.check(SECRET, &code_at(SECRET, NOW), NOW).is_ok());
}

#[test]
fn an_unknown_client_id_behaves_exactly_like_a_wrong_code_and_locks_the_same_way() {
    let guard = TokenGuard::default();
    for attempt in 0..4 {
        assert_eq!(
            guard.check_code("9999999999", None, "123456", NOW + attempt),
            Err(Refusal::Wrong { locked_until: None })
        );
    }
    assert!(matches!(
        guard.check_code("9999999999", None, "123456", NOW + 4),
        Err(Refusal::Wrong {
            locked_until: Some(_)
        })
    ));
    assert!(matches!(
        guard.check_code("9999999999", None, "123456", NOW + 5),
        Err(Refusal::Locked { .. })
    ));

    let decoy_code = code_at(DECOY_SEED, NOW);
    let fresh = TokenGuard::default();
    assert!(
        matches!(
            fresh.check_code("1", None, &decoy_code, NOW),
            Err(Refusal::Wrong { .. })
        ),
        "even the decoy's own code never passes for an unknown client"
    );
}

#[test]
fn one_clients_lockout_does_not_touch_another() {
    let guard = TokenGuard::default();
    for attempt in 0..5 {
        let _ = guard.check_code("attacked", Some(SECRET), "000000", NOW + attempt);
    }
    assert!(
        guard
            .check_code("other", Some(SECRET), &code_at(SECRET, NOW), NOW + 5)
            .is_ok()
    );
}

#[test]
fn each_ip_gets_thirty_requests_a_minute() {
    let guard = TokenGuard::default();
    for second in 0..30 {
        assert!(guard.admit_ip("203.0.113.7", NOW + second / 10));
    }
    assert!(!guard.admit_ip("203.0.113.7", NOW + 5));
    assert!(
        guard.admit_ip("198.51.100.1", NOW + 5),
        "another IP is unaffected"
    );
    assert!(guard.admit_ip("203.0.113.7", NOW + 61), "the window slides");
}

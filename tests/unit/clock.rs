use super::*;

const MONDAY_0000_IST: i64 = 1_792_348_200;

fn at(day_offset: i64, hour: i64, minute: i64) -> i64 {
    MONDAY_0000_IST + day_offset * 86_400 + hour * 3600 + minute * 60
}

fn weekday_window() -> LoginWindow {
    LoginWindow::parse("08:00-08:30", "mon-fri").unwrap()
}

#[test]
fn the_fixture_is_a_monday_at_midnight_ist() {
    assert_eq!(ist_weekday(MONDAY_0000_IST), Weekday::Monday);
    assert_eq!(ist_iso(MONDAY_0000_IST), "2026-10-19T00:00:00+05:30");
}

#[test]
fn the_window_is_open_from_its_start_until_just_before_its_end() {
    let window = weekday_window();
    assert!(!window.is_open(at(0, 7, 59)));
    assert!(window.is_open(at(0, 8, 0)));
    assert!(window.is_open(at(0, 8, 29)));
    assert!(!window.is_open(at(0, 8, 30)));
}

#[test]
fn a_weekend_never_opens_a_weekday_window() {
    let window = weekday_window();
    assert!(!window.is_open(at(5, 8, 10)), "Saturday");
    assert!(!window.is_open(at(6, 8, 10)), "Sunday");
    assert!(
        !window.has_passed_today(at(5, 9, 0)),
        "a non-login day has no missed window"
    );
}

#[test]
fn seconds_until_open_skips_to_the_next_login_day() {
    let window = weekday_window();
    assert_eq!(window.seconds_until_open(at(0, 7, 0)), 3600);
    assert_eq!(window.seconds_until_open(at(0, 8, 10)), 0);
    assert_eq!(window.seconds_until_open(at(0, 9, 0)), 86_400 - 3600);
    assert_eq!(
        window.seconds_until_open(at(4, 9, 0)),
        3 * 86_400 - 3600,
        "after Friday's window the next one is Monday's"
    );
}

#[test]
fn seconds_left_counts_to_the_end_of_an_open_window() {
    let window = weekday_window();
    assert_eq!(window.seconds_left(at(0, 8, 0)), 1800);
    assert_eq!(window.seconds_left(at(0, 8, 29)), 60);
    assert_eq!(window.seconds_left(at(0, 9, 0)), 0);
}

#[test]
fn the_window_has_passed_after_its_end_on_a_login_day() {
    let window = weekday_window();
    assert!(!window.has_passed_today(at(0, 8, 10)));
    assert!(window.has_passed_today(at(0, 8, 30)));
}

#[test]
fn days_accept_lists_ranges_and_full_names() {
    let window = LoginWindow::parse("08:00-08:30", "Monday, wed-thu").unwrap();
    assert!(window.is_login_day(at(0, 8, 0)));
    assert!(!window.is_login_day(at(1, 8, 0)));
    assert!(window.is_login_day(at(2, 8, 0)));
    assert!(window.is_login_day(at(3, 8, 0)));
    assert!(!window.is_login_day(at(4, 8, 0)));
}

#[test]
fn malformed_windows_and_days_are_refused() {
    assert!(LoginWindow::parse("0800-0830", "mon").is_err());
    assert!(LoginWindow::parse("08:30-08:00", "mon").is_err());
    assert!(LoginWindow::parse("25:00-26:00", "mon").is_err());
    assert!(LoginWindow::parse("08:00-08:x", "mon").is_err());
    assert!(LoginWindow::parse("08:00-08:30", "").is_err());
    assert!(LoginWindow::parse("08:00-08:30", "fri-mon").is_err());
    assert!(LoginWindow::parse("08:00-08:30", "funday").is_err());
}

#[test]
fn the_label_and_day_number_are_in_ist() {
    assert_eq!(weekday_window().label(), "08:00-08:30 IST");
    assert_eq!(ist_day_number(at(0, 0, 0)), ist_day_number(at(0, 23, 59)));
    assert_ne!(ist_day_number(at(0, 23, 59)), ist_day_number(at(1, 0, 0)));
}

#[test]
fn durations_read_naturally() {
    assert_eq!(describe(5), "5s");
    assert_eq!(describe(125), "2m 5s");
    assert_eq!(describe(3 * 3600 + 120), "3h 2m");
    assert_eq!(describe(-4), "0s");
}

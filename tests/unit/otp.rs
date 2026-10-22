use super::*;

const RFC_SECRET: &[u8] = b"12345678901234567890";

#[test]
fn matches_the_rfc_6238_sha1_test_vectors() {
    let cases = [
        (59u64, "287082"),
        (1_111_111_109, "081804"),
        (1_111_111_111, "050471"),
        (1_234_567_890, "005924"),
        (2_000_000_000, "279037"),
        (20_000_000_000, "353130"),
    ];
    for (at, expected) in cases {
        assert_eq!(
            code_at(RFC_SECRET, at),
            expected,
            "RFC 6238 vector for t={at}"
        );
    }
}

#[test]
fn a_code_is_always_six_digits_including_leading_zeros() {
    let code = code_at(RFC_SECRET, 1_234_567_890);
    assert_eq!(code, "005924");
    assert!(is_six_digits(&code));
}

#[test]
fn the_code_is_stable_inside_a_step_and_changes_across_one() {
    let base = 1_111_111_110;
    let step_start = base - (base % 30);
    let first = code_at(RFC_SECRET, step_start);
    assert_eq!(code_at(RFC_SECRET, step_start + 29), first);
    assert_ne!(code_at(RFC_SECRET, step_start + 30), first);
}

#[test]
fn seconds_remaining_counts_down_to_the_window_edge() {
    let step_start = 1_700_000_000 - (1_700_000_000 % 30);
    assert_eq!(seconds_remaining(step_start), 30);
    assert_eq!(seconds_remaining(step_start + 1), 29);
    assert_eq!(seconds_remaining(step_start + 29), 1);
}

#[test]
fn base32_decodes_the_rfc_4648_alphabet_and_ignores_padding_and_spaces() {
    assert_eq!(
        decode_base32("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ").unwrap(),
        RFC_SECRET
    );
    assert_eq!(
        decode_base32("gezd gnbv gy3t qojq gezd gnbv gy3t qojq").unwrap(),
        RFC_SECRET,
        "an authenticator shows the seed in spaced lowercase groups"
    );
    assert_eq!(decode_base32("MFRGG===").unwrap(), b"abc");
}

#[test]
fn base32_refuses_input_that_is_not_base32() {
    assert!(decode_base32("MFRGG1").is_err());
    assert!(decode_base32("MFRGG!").is_err());
    assert!(
        decode_base32("").is_err(),
        "an empty secret must not silently work"
    );
    assert!(decode_base32("====").is_err());
}

#[test]
fn a_step_code_equals_the_code_at_any_second_inside_that_step() {
    let at = 1_111_111_109;
    assert_eq!(
        code_for_step(RFC_SECRET, step_of(at)),
        code_at(RFC_SECRET, at)
    );
}

#[test]
fn six_digit_check_refuses_anything_else() {
    assert!(is_six_digits("000000"));
    assert!(!is_six_digits("12345"));
    assert!(!is_six_digits("1234567"));
    assert!(!is_six_digits("12345a"));
    assert!(!is_six_digits("１２３４５６"));
}

#[test]
fn now_is_after_2026() {
    assert!(now_seconds() > 1_767_225_600);
}

use super::*;

const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OTHER_KEY: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

#[test]
fn a_token_round_trips_under_its_own_context() {
    let cipher = Cipher::from_hex(KEY).unwrap();
    let sealed = cipher
        .encrypt("eyJhbGciOi.token", "main:1100000001")
        .unwrap();
    assert!(sealed.starts_with("v1."));
    assert!(
        !sealed.contains("eyJhbGciOi"),
        "the plaintext must not appear in the stored value"
    );
    assert_eq!(
        cipher.decrypt(&sealed, "main:1100000001").unwrap(),
        "eyJhbGciOi.token"
    );
}

#[test]
fn every_encryption_uses_a_fresh_nonce() {
    let cipher = Cipher::from_hex(KEY).unwrap();
    let first = cipher.encrypt("same", "ctx").unwrap();
    let second = cipher.encrypt("same", "ctx").unwrap();
    assert_ne!(first, second);
}

#[test]
fn a_row_moved_to_another_account_does_not_decrypt() {
    let cipher = Cipher::from_hex(KEY).unwrap();
    let sealed = cipher.encrypt("token", "main:1100000001").unwrap();
    assert!(
        cipher.decrypt(&sealed, "other:1100000002").is_err(),
        "the account is bound as associated data, so a swapped row must fail"
    );
}

#[test]
fn the_wrong_key_fails_authentication_rather_than_returning_garbage() {
    let sealed = Cipher::from_hex(KEY)
        .unwrap()
        .encrypt("token", "ctx")
        .unwrap();
    let error = Cipher::from_hex(OTHER_KEY)
        .unwrap()
        .decrypt(&sealed, "ctx")
        .unwrap_err();
    assert!(error.to_string().contains("failed authentication"));
}

#[test]
fn a_tampered_or_malformed_value_is_refused() {
    let cipher = Cipher::from_hex(KEY).unwrap();
    let sealed = cipher.encrypt("token", "ctx").unwrap();
    let mut tampered = sealed.clone();
    tampered.pop();
    tampered.push(if sealed.ends_with('A') { 'B' } else { 'A' });
    assert!(cipher.decrypt(&tampered, "ctx").is_err());
    assert!(cipher.decrypt("v1.only", "ctx").is_err());
    assert!(cipher.decrypt("v2.a.b", "ctx").is_err());
    assert!(cipher.decrypt("v1.!!.b", "ctx").is_err());
    assert!(cipher.decrypt("v1.AAAA.b", "ctx").is_err());
    assert!(cipher.decrypt("v1.a.b.c", "ctx").is_err());
}

#[test]
fn the_key_must_be_exactly_64_hex_characters() {
    assert!(Cipher::from_hex("").is_err());
    assert!(Cipher::from_hex(&KEY[..63]).is_err());
    assert!(Cipher::from_hex(&format!("{}g", &KEY[..63])).is_err());
    assert!(
        Cipher::from_hex(&format!(" {KEY} ")).is_ok(),
        "surrounding whitespace is tolerated"
    );
}

#[test]
fn sha256_matches_a_known_vector() {
    assert_eq!(
        sha256_hex("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn constant_time_eq_compares_values_not_lengths() {
    assert!(constant_time_eq("123456", "123456"));
    assert!(!constant_time_eq("123456", "123457"));
    assert!(!constant_time_eq("123456", "1234567"));
}

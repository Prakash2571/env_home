use std::sync::atomic::Ordering;

use serde_json::json;

use super::*;
use crate::test_support::{ACCESS_AUD, AccessSigner, access_team};

const NOW: i64 = 1_800_000_000;

async fn verifier_and_signer() -> (AccessVerifier, AccessSigner, String) {
    let signer = AccessSigner::new("key-1");
    let (team, _) = access_team(&signer).await;
    (AccessVerifier::new(&team).unwrap(), signer, team)
}

#[tokio::test]
async fn a_signed_in_person_is_identified_by_email() {
    let (verifier, signer, team) = verifier_and_signer().await;
    let token = signer.person(&team, ACCESS_AUD, "Me@Example.com", NOW);
    let identity = verifier.verify(&token, ACCESS_AUD, NOW + 10).await.unwrap();
    assert_eq!(identity.email.as_deref(), Some("me@example.com"));
    assert_eq!(identity.common_name, None);
}

#[tokio::test]
async fn a_service_token_is_identified_by_its_common_name() {
    let (verifier, signer, team) = verifier_and_signer().await;
    let token = signer.service(&team, ACCESS_AUD, NOW);
    let identity = verifier.verify(&token, ACCESS_AUD, NOW).await.unwrap();
    assert_eq!(identity.email, None);
    assert_eq!(identity.common_name.as_deref(), Some("engine.access"));
}

#[tokio::test]
async fn a_token_for_another_app_team_or_time_is_refused() {
    let (verifier, signer, team) = verifier_and_signer().await;
    let person = |issuer: &str, audience: &str| signer.person(issuer, audience, "me@x.in", NOW);
    assert_eq!(
        verifier
            .verify(&person(&team, "other-aud"), ACCESS_AUD, NOW)
            .await,
        Err(AccessError::WrongAudience)
    );
    assert_eq!(
        verifier
            .verify(
                &person("https://evil.cloudflareaccess.com", ACCESS_AUD),
                ACCESS_AUD,
                NOW
            )
            .await,
        Err(AccessError::WrongIssuer)
    );
    let token = person(&team, ACCESS_AUD);
    assert_eq!(
        verifier
            .verify(&token, ACCESS_AUD, NOW + 3600 + LEEWAY_SECONDS + 1)
            .await,
        Err(AccessError::Expired)
    );
    assert_eq!(
        verifier
            .verify(&token, ACCESS_AUD, NOW - LEEWAY_SECONDS - 1)
            .await,
        Err(AccessError::NotYetValid)
    );
    assert!(
        verifier
            .verify(&token, ACCESS_AUD, NOW + 3600 + LEEWAY_SECONDS)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn a_tampered_or_unsigned_token_never_passes() {
    let (verifier, signer, team) = verifier_and_signer().await;
    let token = signer.person(&team, ACCESS_AUD, "me@x.in", NOW);
    let (signed, _) = token.rsplit_once('.').unwrap();
    let (header, _) = signed.split_once('.').unwrap();
    let forged_claims = {
        use base64::Engine;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
            json!({"aud": [ACCESS_AUD], "iss": team, "email": "boss@x.in", "exp": NOW + 3600})
                .to_string(),
        )
    };
    let signature = token.rsplit_once('.').unwrap().1;
    let forged = format!("{header}.{forged_claims}.{signature}");
    assert_eq!(
        verifier.verify(&forged, ACCESS_AUD, NOW).await,
        Err(AccessError::BadSignature)
    );

    let none_alg = signer.sign_with(&json!({"alg": "none", "kid": "key-1"}), &json!({}));
    assert_eq!(
        verifier.verify(&none_alg, ACCESS_AUD, NOW).await,
        Err(AccessError::Malformed)
    );
    let hs256 = signer.sign_with(&json!({"alg": "HS256", "kid": "key-1"}), &json!({}));
    assert_eq!(
        verifier.verify(&hs256, ACCESS_AUD, NOW).await,
        Err(AccessError::Malformed)
    );
    for junk in ["", "a.b", "a.b.c.d", "!!.??.**"] {
        assert_eq!(
            verifier.verify(junk, ACCESS_AUD, NOW).await,
            Err(AccessError::Malformed),
            "{junk:?}"
        );
    }
}

#[tokio::test]
async fn certs_are_cached_and_an_unknown_key_refetches_at_most_every_30_seconds() {
    let signer = AccessSigner::new("key-1");
    let (team, fetches) = access_team(&signer).await;
    let verifier = AccessVerifier::new(&team).unwrap();
    let token = signer.person(&team, ACCESS_AUD, "me@x.in", NOW);
    for _ in 0..3 {
        verifier.verify(&token, ACCESS_AUD, NOW).await.unwrap();
    }
    assert_eq!(
        fetches.load(Ordering::SeqCst),
        1,
        "one fetch serves every request"
    );

    let rotated = AccessSigner::new("key-2");
    let unknown = rotated.person(&team, ACCESS_AUD, "me@x.in", NOW);
    for _ in 0..5 {
        assert_eq!(
            verifier.verify(&unknown, ACCESS_AUD, NOW).await,
            Err(AccessError::UnknownKey)
        );
    }
    assert_eq!(
        fetches.load(Ordering::SeqCst),
        1,
        "an unknown kid right after a fetch does not hammer the team domain"
    );
}

#[tokio::test]
async fn an_unreachable_team_domain_refuses_rather_than_trusting() {
    let signer = AccessSigner::new("key-1");
    let verifier = AccessVerifier::new("http://127.0.0.1:1").unwrap();
    let token = signer.person("http://127.0.0.1:1", ACCESS_AUD, "me@x.in", NOW);
    assert_eq!(
        verifier.verify(&token, ACCESS_AUD, NOW).await,
        Err(AccessError::KeysUnavailable)
    );
    assert!(format!("{verifier:?}").contains("127.0.0.1:1"));
}

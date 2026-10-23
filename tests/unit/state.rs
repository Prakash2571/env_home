use super::*;
use crate::test_support::account;

#[test]
fn every_account_starts_idle_with_the_full_stage_chain_and_no_secrets() {
    let state = LiveState::new(&[account("main", "1100000001")]);
    let view = state.account("main").unwrap();
    assert_eq!(view.phase, Phase::Idle);
    assert_eq!(view.stages.len(), STAGES.len());
    assert!(
        view.stages
            .iter()
            .all(|stage| stage.status == StageStatus::Idle)
    );
    assert_eq!(
        view.client_id_hint, "••••0001",
        "the view never carries the whole client ID"
    );
    assert!(state.account("missing").is_none());
}

#[test]
fn a_stage_records_when_it_ran_and_how_it_ended() {
    let state = LiveState::new(&[account("main", "1")]);
    state.stage("main", StageId::Consent, StageStatus::Running, "");
    let running = state.account("main").unwrap().stages[0].clone();
    assert!(running.started_at.is_some() && running.finished_at.is_none());

    state.stage("main", StageId::Consent, StageStatus::Done, "generated");
    let done = state.account("main").unwrap().stages[0].clone();
    assert_eq!(done.status, StageStatus::Done);
    assert_eq!(done.detail, "generated");
    assert!(done.finished_at.is_some());

    state.stage("main", StageId::Browser, StageStatus::Skipped, "manual");
    let skipped = state.account("main").unwrap().stages[1].clone();
    assert_eq!(
        skipped.started_at, skipped.finished_at,
        "a skipped stage starts and ends at once"
    );
}

#[test]
fn a_new_attempt_clears_the_previous_one() {
    let state = LiveState::new(&[account("main", "1")]);
    state.update("main", |view| {
        view.last_error = Some("old".into());
        view.consent_url = Some("https://old".into());
    });
    state.stage("main", StageId::Exchange, StageStatus::Failed, "boom");
    state.begin_attempt("main");
    let view = state.account("main").unwrap();
    assert_eq!(view.phase, Phase::LoggingIn);
    assert!(view.last_error.is_none() && view.consent_url.is_none());
    assert!(
        view.stages
            .iter()
            .all(|stage| stage.status == StageStatus::Idle)
    );
}

#[test]
fn events_are_newest_first_and_bounded() {
    let state = LiveState::new(&[]);
    for index in 0..205 {
        state.note(EventKind::System, None, format!("event {index}"));
    }
    let events = state.events();
    assert_eq!(events.len(), 200);
    assert_eq!(events[0].message, "event 204");
}

#[tokio::test]
async fn every_change_bumps_the_revision_the_stream_waits_on() {
    let state = LiveState::new(&[account("main", "1")]);
    let mut revisions = state.subscribe();
    let before = *revisions.borrow_and_update();
    state.note(EventKind::Login, Some("main"), "hello");
    revisions.changed().await.unwrap();
    assert!(*revisions.borrow() > before);
}

#[test]
fn short_client_ids_are_hinted_without_panicking() {
    assert_eq!(client_id_hint("12"), "••••12");
    assert_eq!(client_id_hint(""), "••••");
}

#[test]
fn accounts_can_join_and_leave_the_dashboard_while_it_runs() {
    let state = LiveState::new(&[]);
    let mut changes = state.subscribe();
    state.add_account(&account("main", "1100000001"));
    state.add_account(&account("main", "1100000001"));
    assert_eq!(state.accounts().len(), 1, "adding twice shows it once");
    assert!(changes.has_changed().unwrap());
    changes.mark_unchanged();
    state.remove_account("main");
    assert!(state.accounts().is_empty());
    assert!(changes.has_changed().unwrap());
}

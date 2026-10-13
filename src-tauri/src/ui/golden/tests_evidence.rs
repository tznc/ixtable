//! Port of `tests/integration/journey-evidence.test.tsx`: a failed journey writes its error,
//! reproduction steps and the log tail; a passing one writes no log tail.

use super::journey::{evidence_dir, with_journey};
use serde_json::Value;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;

// Both tests write the same evidence file.
static LOCK: Mutex<()> = Mutex::new(());

fn path() -> std::path::PathBuf {
    evidence_dir().join("evidence-check-fails.json")
}

fn written() -> Value {
    serde_json::from_str(&std::fs::read_to_string(path()).expect("evidence file")).expect("json")
}

#[test]
fn golden_a_failed_journey_writes_its_error_reproduction_steps_and_the_log_tail() {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        with_journey("evidence-check", "fails", |journey| {
            journey.step("open the app", |_| {});
            journey.step("break", |_| panic!("boom"));
        })
    }));
    let message = outcome
        .err()
        .and_then(|p| p.downcast_ref::<&str>().map(|s| s.to_string()))
        .expect("the journey rethrows");
    assert_eq!(message, "boom");
    assert!(path().exists());
    let doc = written();
    assert_eq!(doc["status"], "failed");
    assert_eq!(doc["error"], "boom");
    assert_eq!(doc["reproduction"], serde_json::json!(["1. open the app", "2. break"]));
    assert!(doc["logTail"].is_array());
    let _ = std::fs::remove_file(path());
}

#[test]
fn golden_a_passing_journey_writes_no_log_tail() {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    with_journey("evidence-check", "fails", |journey| {
        journey.step("open the app", |_| {});
    });
    let doc = written();
    assert_eq!(doc["status"], "passed");
    assert!(doc.get("logTail").is_none());
    let _ = std::fs::remove_file(path());
}

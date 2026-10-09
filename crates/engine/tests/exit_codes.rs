//! Spec §8 exit codes: 3 fatal > 2 usage/config > 1 partial > 4 validation failed > 0 success, and DECISIONS
//! Note I (intentional skips do not make a batch partial).
use engine::api::*;

#[test]
fn precedence_partial_over_validation_over_success() {
    let mut s = BatchSummary::default();
    assert_eq!(s.exit_code(), 0);
    s.converted = 5;
    s.up_to_date = 3;
    assert_eq!(s.exit_code(), 0);
    s.validation_failed = 1;
    assert_eq!(s.exit_code(), 4);
    // Intentional skips (Note I) leave 4 alone; an unintended skip makes the batch partial.
    (s.skipped, s.skipped_intentional) = (2, 2);
    assert_eq!(s.exit_code(), 4);
    s.skipped = 3;
    assert_eq!(s.exit_code(), 1);
    // Each partial cause outranks 4 by itself.
    for mutate in [
        (|s: &mut BatchSummary| s.failed = 1) as fn(&mut BatchSummary),
        |s| s.cancelled.push("a.png".into()),
    ] {
        let mut s = BatchSummary {
            validation_failed: 1,
            ..Default::default()
        };
        assert_eq!(s.exit_code(), 4);
        mutate(&mut s);
        assert_eq!(s.exit_code(), 1);
    }
    assert_eq!(EngineError::Usage("x".into()).exit_code(), 2);
    assert_eq!(EngineError::Config("x".into()).exit_code(), 2);
    assert_eq!(EngineError::Fatal("x".into()).exit_code(), 3);
}

#[test]
fn cancelled_alone_is_partial_and_intentional_skips_alone_are_success() {
    let s = BatchSummary {
        cancelled: vec!["a.png".into()],
        ..Default::default()
    };
    assert_eq!(s.exit_code(), 1);
    let s = BatchSummary {
        skipped: 4,
        skipped_intentional: 4,
        ..Default::default()
    };
    assert_eq!(s.exit_code(), 0);
}

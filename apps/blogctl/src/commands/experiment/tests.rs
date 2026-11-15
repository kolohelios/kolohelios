use super::*;
use std::io::Cursor;

#[test]
fn review_confirmation_requires_the_specific_decision() {
    for (decision, answer) in [
        (writing::ReviewDecision::Accept, "ACCEPT\n"),
        (writing::ReviewDecision::Reject, "REJECT\n"),
    ] {
        let mut output = Vec::new();
        confirm_review(decision, "synthetic-hash", Cursor::new(answer), &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("synthetic-hash"));
        assert!(output.contains("does NOT promote"));
    }
    for answer in ["", "yes\n", "REJECT\n", " ACCEPT\n", "ACCEPT please\n"] {
        assert!(confirm_review(
            writing::ReviewDecision::Accept,
            "hash",
            Cursor::new(answer),
            &mut Vec::new()
        )
        .is_err());
    }
}

#[test]
fn polished_confirmation_requires_the_specific_phrase() {
    for (decision, answer, phrase) in [
        (
            crate::writing_polished::PolishedDecision::Approve,
            "APPROVE POLISHED\n",
            "APPROVE POLISHED",
        ),
        (
            crate::writing_polished::PolishedDecision::Reject,
            "REJECT POLISHED\n",
            "REJECT POLISHED",
        ),
    ] {
        let mut output = Vec::new();
        confirm_polished(decision, "synthetic-hash", Cursor::new(answer), &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("synthetic-hash"));
        assert!(output.contains(phrase));
    }
    for answer in ["", "APPROVE\n", "POLISHED\n", " APPROVE POLISHED\n"] {
        assert!(confirm_polished(
            crate::writing_polished::PolishedDecision::Approve,
            "hash",
            Cursor::new(answer),
            &mut Vec::new()
        )
        .is_err());
    }
}

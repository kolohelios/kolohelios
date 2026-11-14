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

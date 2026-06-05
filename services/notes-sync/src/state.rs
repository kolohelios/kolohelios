//! Pure, native-testable note-state logic: folding the append-only edit
//! log back into text (the lossless-rehydrate guarantee) and the
//! stale-edit predicate. Kept off the wasm-only path so `cargo test`
//! exercises it on the host — these are the parts that lose data or
//! admit a wrong edit when wrong.

use notes_protocol::{ApplyError, Delta, Seq};

/// Replay an ordered slice of accepted deltas from the empty document,
/// reconstructing the current body. This is the lossless-rehydrate path:
/// an evicted Durable Object can rebuild its text from the log alone, so
/// the materialized snapshot is only ever an optimization, never the
/// source of truth.
pub fn replay(deltas: &[Delta]) -> Result<String, ApplyError> {
    let mut text = String::new();
    for delta in deltas {
        text = delta.apply(&text)?;
    }
    Ok(text)
}

/// An edit is stale when its `base_seq` is not the server's current
/// `seq` — the client raced another accepted edit and must resync before
/// retrying. This is the check that keeps two writers from silently
/// clobbering each other.
pub fn is_stale(base_seq: Seq, current_seq: Seq) -> bool {
    base_seq != current_seq
}

/// The time the single DO alarm should fire next: the earliest of the
/// (optional) commit deadlines. A Durable Object has one alarm primitive,
/// so the lazy git commit multiplexes its `debounce` deadline (commit
/// shortly after the last edit, coalescing a burst) and its `backstop`
/// deadline (commit at least every N seconds under continuous editing)
/// onto it. `None` when neither is pending — nothing to commit.
pub fn next_alarm(debounce_due: Option<i64>, backstop_due: Option<i64>) -> Option<i64> {
    match (debounce_due, backstop_due) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_folds_deltas_from_empty() {
        let log = [
            Delta::Whole {
                text: "hello".into(),
            },
            Delta::Splice {
                at: 5,
                remove: 0,
                insert: " world".into(),
            },
            Delta::Splice {
                at: 0,
                remove: 5,
                insert: "goodbye".into(),
            },
        ];
        assert_eq!(replay(&log).unwrap(), "goodbye world");
    }

    #[test]
    fn replay_empty_log_is_empty() {
        assert_eq!(replay(&[]).unwrap(), "");
    }

    #[test]
    fn replay_surfaces_a_bad_delta() {
        let log = [Delta::Splice {
            at: 10,
            remove: 0,
            insert: "x".into(),
        }];
        assert!(replay(&log).is_err());
    }

    #[test]
    fn stale_when_base_seq_does_not_match_current() {
        assert!(is_stale(2, 3)); // behind
        assert!(is_stale(4, 3)); // ahead (also a mismatch)
        assert!(!is_stale(3, 3)); // exact match is fresh
    }

    #[test]
    fn next_alarm_is_the_earlier_deadline() {
        assert_eq!(next_alarm(Some(100), Some(250)), Some(100));
        assert_eq!(next_alarm(Some(300), Some(250)), Some(250));
    }

    #[test]
    fn next_alarm_uses_whichever_deadline_is_set() {
        assert_eq!(next_alarm(Some(100), None), Some(100));
        assert_eq!(next_alarm(None, Some(250)), Some(250));
    }

    #[test]
    fn next_alarm_is_none_when_nothing_pending() {
        assert_eq!(next_alarm(None, None), None);
    }
}

use serde::Serialize;

use crate::term::{BOLD, RED, RESET};

/// Emit a command result either as pretty-printed JSON (when `json` is
/// true) or via the provided human renderer.
///
/// Centralizes the json/human switch every structured-output command
/// shares: in JSON mode a serialization failure prints to stderr and
/// exits non-zero, matching the error convention used across `shaka`.
/// In human mode the closure renders as before.
pub fn emit<T: Serialize>(json: bool, value: &T, render_human: impl FnOnce(&T)) {
    if json {
        match serde_json::to_string_pretty(value) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("{RED}{BOLD}error:{RESET} failed to serialize output: {e}");
                std::process::exit(1);
            }
        }
    } else {
        render_human(value);
    }
}

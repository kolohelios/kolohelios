//! Typed engine errors. These surface to the client as `400` JSON
//! (`{"error": "...", "detail": "<Display>"}`) at the Worker boundary, so
//! every `display` string is user-facing.

use snafu::Snafu;

#[derive(Debug, Snafu, PartialEq, Eq)]
#[snafu(visibility(pub))]
pub enum EngineError {
    #[snafu(display(
        "couldn't read '{token}' — expected a card number or one of the named cards for this game"
    ))]
    UnknownToken { token: String },

    #[snafu(display("'{player}' isn't a player in this game"))]
    UnknownPlayer { player: String },

    #[snafu(display("this game doesn't track a per-round award"))]
    AwardNotSupported,

    #[snafu(display("nothing to undo"))]
    NothingToUndo,

    #[snafu(display("this game is complete — no more rounds"))]
    GameComplete,

    #[snafu(display("that action doesn't match this game's scoring style"))]
    WrongModel,

    #[snafu(display("{detail}"))]
    BadRequest { detail: String },
}

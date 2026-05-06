use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    Concept,
    Ideation,
    Editing,
    FinalEditing,
    Published,
    Abandoned,
}

impl Stage {
    pub const ALL: &'static [Stage] = &[
        Stage::Concept,
        Stage::Ideation,
        Stage::Editing,
        Stage::FinalEditing,
        Stage::Published,
        Stage::Abandoned,
    ];

    pub fn dirname(self) -> &'static str {
        match self {
            Stage::Concept => "concepts",
            Stage::Ideation => "ideation",
            Stage::Editing => "editing",
            Stage::FinalEditing => "final-editing",
            Stage::Published => "published",
            Stage::Abandoned => "abandoned",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Concept => "concept",
            Stage::Ideation => "ideation",
            Stage::Editing => "editing",
            Stage::FinalEditing => "final-editing",
            Stage::Published => "published",
            Stage::Abandoned => "abandoned",
        }
    }

    pub fn from_dirname(dir: &str) -> Option<Self> {
        Stage::ALL.iter().copied().find(|s| s.dirname() == dir)
    }

    /// Promote along the linear workflow: concept → ideation → editing →
    /// final-editing → published. Abandoned and Published are terminal.
    pub fn promote(self) -> Result<Stage> {
        match self {
            Stage::Concept => Ok(Stage::Ideation),
            Stage::Ideation => Ok(Stage::Editing),
            Stage::Editing => Ok(Stage::FinalEditing),
            Stage::FinalEditing => Ok(Stage::Published),
            Stage::Published | Stage::Abandoned => Err(Error::PromoteFromTerminal(self)),
        }
    }

    /// Demote one step back. Concept can't demote; Published refuses
    /// without an explicit force (a future flag); Abandoned can't demote.
    pub fn demote(self) -> Result<Stage> {
        match self {
            Stage::Concept => Err(Error::DemoteFromInitial(self)),
            Stage::Ideation => Ok(Stage::Concept),
            Stage::Editing => Ok(Stage::Ideation),
            Stage::FinalEditing => Ok(Stage::Editing),
            Stage::Published => Err(Error::DemotePublished),
            Stage::Abandoned => Err(Error::DemoteFromInitial(self)),
        }
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Stage {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "concept" => Ok(Stage::Concept),
            "ideation" => Ok(Stage::Ideation),
            "editing" => Ok(Stage::Editing),
            "final-editing" => Ok(Stage::FinalEditing),
            "published" => Ok(Stage::Published),
            "abandoned" => Ok(Stage::Abandoned),
            other => Err(Error::InvalidStage(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn promote_walks_the_linear_workflow() {
        assert_eq!(Stage::Concept.promote().unwrap(), Stage::Ideation);
        assert_eq!(Stage::Ideation.promote().unwrap(), Stage::Editing);
        assert_eq!(Stage::Editing.promote().unwrap(), Stage::FinalEditing);
        assert_eq!(Stage::FinalEditing.promote().unwrap(), Stage::Published);
    }

    #[test]
    fn promote_from_published_is_terminal() {
        let err = Stage::Published.promote().unwrap_err();
        assert!(matches!(err, Error::PromoteFromTerminal(Stage::Published)));
    }

    #[test]
    fn promote_from_abandoned_is_terminal() {
        let err = Stage::Abandoned.promote().unwrap_err();
        assert!(matches!(err, Error::PromoteFromTerminal(Stage::Abandoned)));
    }

    #[test]
    fn demote_walks_back_to_concept() {
        assert_eq!(Stage::Ideation.demote().unwrap(), Stage::Concept);
        assert_eq!(Stage::Editing.demote().unwrap(), Stage::Ideation);
        assert_eq!(Stage::FinalEditing.demote().unwrap(), Stage::Editing);
    }

    #[test]
    fn demote_from_concept_is_initial() {
        let err = Stage::Concept.demote().unwrap_err();
        assert!(matches!(err, Error::DemoteFromInitial(Stage::Concept)));
    }

    #[test]
    fn demote_from_published_is_blocked_pending_force() {
        let err = Stage::Published.demote().unwrap_err();
        assert!(matches!(err, Error::DemotePublished));
    }

    #[test]
    fn parse_round_trips_for_each_stage() {
        for &s in Stage::ALL {
            assert_eq!(s.as_str().parse::<Stage>().unwrap(), s);
        }
    }

    #[test]
    fn parse_rejects_unknown_stage() {
        assert!("draft".parse::<Stage>().is_err());
    }

    #[test]
    fn from_dirname_round_trips() {
        for &s in Stage::ALL {
            assert_eq!(Stage::from_dirname(s.dirname()), Some(s));
        }
        assert_eq!(Stage::from_dirname("history"), None);
    }
}

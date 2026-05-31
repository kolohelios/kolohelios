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

    /// Linear position in the editorial pipeline: `Concept` is 0,
    /// `Published` is 4. `Abandoned` sits off the pipeline and returns
    /// `None` — callers that compare ranks (e.g. completeness checks)
    /// must handle the bypass explicitly.
    pub fn pipeline_rank(self) -> Option<u8> {
        match self {
            Stage::Concept => Some(0),
            Stage::Ideation => Some(1),
            Stage::Editing => Some(2),
            Stage::FinalEditing => Some(3),
            Stage::Published => Some(4),
            Stage::Abandoned => None,
        }
    }

    /// True when a `required_by = <threshold>` rule kicks in for a post
    /// at `self`. `Abandoned` posts always bypass (they're being killed,
    /// no need to enforce classification); otherwise the post has to
    /// have reached `threshold` (in pipeline order).
    pub fn triggers_required_by(self, threshold: Stage) -> bool {
        match (self.pipeline_rank(), threshold.pipeline_rank()) {
            (Some(here), Some(at)) => here >= at,
            // self is abandoned → always bypass.
            // threshold is abandoned → never triggers (declaring
            // `required_by = "abandoned"` is meaningless config, not
            // an error).
            _ => false,
        }
    }

    /// Promote along the linear workflow: concept → ideation → editing →
    /// final-editing → published. Abandoned and Published are terminal.
    pub fn promote(self) -> Result<Stage> {
        match self {
            Stage::Concept => Ok(Stage::Ideation),
            Stage::Ideation => Ok(Stage::Editing),
            Stage::Editing => Ok(Stage::FinalEditing),
            Stage::FinalEditing => Ok(Stage::Published),
            Stage::Published | Stage::Abandoned => Err(Error::PromoteFromTerminal { stage: self }),
        }
    }

    /// Demote one step back. Concept can't demote; Published refuses
    /// without an explicit force (a future flag); Abandoned can't demote.
    pub fn demote(self) -> Result<Stage> {
        match self {
            Stage::Concept => Err(Error::DemoteFromInitial { stage: self }),
            Stage::Ideation => Ok(Stage::Concept),
            Stage::Editing => Ok(Stage::Ideation),
            Stage::FinalEditing => Ok(Stage::Editing),
            Stage::Published => Err(Error::DemotePublished),
            Stage::Abandoned => Err(Error::DemoteFromInitial { stage: self }),
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
            other => Err(Error::InvalidStage {
                value: other.to_string(),
            }),
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
        assert!(matches!(
            err,
            Error::PromoteFromTerminal {
                stage: Stage::Published
            }
        ));
    }

    #[test]
    fn promote_from_abandoned_is_terminal() {
        let err = Stage::Abandoned.promote().unwrap_err();
        assert!(matches!(
            err,
            Error::PromoteFromTerminal {
                stage: Stage::Abandoned
            }
        ));
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
        assert!(matches!(
            err,
            Error::DemoteFromInitial {
                stage: Stage::Concept
            }
        ));
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

    #[test]
    fn pipeline_rank_orders_the_linear_stages() {
        assert!(Stage::Concept.pipeline_rank() < Stage::Ideation.pipeline_rank());
        assert!(Stage::Ideation.pipeline_rank() < Stage::Editing.pipeline_rank());
        assert!(Stage::Editing.pipeline_rank() < Stage::FinalEditing.pipeline_rank());
        assert!(Stage::FinalEditing.pipeline_rank() < Stage::Published.pipeline_rank());
    }

    #[test]
    fn pipeline_rank_is_none_for_abandoned() {
        assert!(Stage::Abandoned.pipeline_rank().is_none());
    }

    #[test]
    fn triggers_required_by_when_post_has_reached_threshold() {
        // required_by = "editing" → editing, final-editing, and
        // published all trigger.
        assert!(Stage::Editing.triggers_required_by(Stage::Editing));
        assert!(Stage::FinalEditing.triggers_required_by(Stage::Editing));
        assert!(Stage::Published.triggers_required_by(Stage::Editing));
        // …but earlier stages don't.
        assert!(!Stage::Concept.triggers_required_by(Stage::Editing));
        assert!(!Stage::Ideation.triggers_required_by(Stage::Editing));
    }

    #[test]
    fn abandoned_posts_bypass_required_by_completely() {
        // Regardless of threshold, an abandoned post never triggers
        // required-ness checks — the post is being killed.
        for &threshold in Stage::ALL {
            assert!(!Stage::Abandoned.triggers_required_by(threshold));
        }
    }

    #[test]
    fn required_by_abandoned_is_meaningless_config() {
        // Declaring `required_by = "abandoned"` never triggers for
        // any post (no post needs to "reach abandoned"). Doesn't
        // error; just a no-op rule.
        for &here in Stage::ALL {
            assert!(!here.triggers_required_by(Stage::Abandoned));
        }
    }
}

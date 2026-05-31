//! Declarative classification taxonomy. The set of allowed values per
//! dimension lives in `.blog-os.toml` under `[classifications.*]`;
//! this module loads it into a `Taxonomy` and runs the validation
//! pass against `Classifications` instances.
//!
//! The taxonomy is data, not code — adding a new format or motif
//! is a config edit, not a Rust change. The struct's *dimension
//! names* (`format`, `hook`, `tone`, `audience`, `motifs`) come from
//! `Classifications`'s field names and are fixed; the *values*
//! inside each dimension are user-configurable.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::stage::Stage;

/// One dimension of the taxonomy: a list of allowed values, a `multi`
/// flag documenting the cardinality, and an optional `required_by`
/// stage threshold for the completeness check.
///
/// `multi` is informational — the Rust type system carries the
/// single/multi distinction (each `Classifications` field is either
/// `Option<String>` or `Vec<String>`), so `multi = true` here is
/// self-documentation for the taxonomy file.
///
/// `required_by` activates the completeness check (see
/// `Classifications::missing_at_stage`): the dimension must be set on
/// posts whose status has reached the named stage. Absence means
/// "never required" — the previous behavior before #602.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dimension {
    #[serde(default)]
    pub multi: bool,
    pub values: Vec<String>,
    /// Stage at (and beyond) which the dimension is required. `None`
    /// means the dimension is always optional. `Some(Stage::Abandoned)`
    /// is a meaningless config — no post ever needs to "reach
    /// abandoned" — but parses cleanly and is a no-op at check time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_by: Option<Stage>,
}

impl Dimension {
    /// True when `value` is one of the dimension's declared options.
    pub fn allows(&self, value: &str) -> bool {
        self.values.iter().any(|v| v == value)
    }
}

/// The full taxonomy — a map from dimension name to its allowed
/// values. Wraps a `BTreeMap` so the `from(config.classifications)`
/// path is direct and call sites stay readable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Taxonomy {
    dimensions: BTreeMap<String, Dimension>,
}

impl Taxonomy {
    pub fn new(dimensions: BTreeMap<String, Dimension>) -> Self {
        Self { dimensions }
    }

    /// Look up a dimension by name. `None` means "not declared" —
    /// validation is permissive on undeclared dimensions so the user
    /// can roll out (or remove) a dimension from their taxonomy
    /// without invalidating every post.
    pub fn dimension(&self, name: &str) -> Option<&Dimension> {
        self.dimensions.get(name)
    }

    pub fn is_empty(&self) -> bool {
        self.dimensions.is_empty()
    }

    /// The v1 taxonomy baked into `init`. Mirrors the table in #437's
    /// issue body. Edit here when adding a starter value.
    pub fn current_v1() -> Self {
        Self {
            dimensions: starter_v1(),
        }
    }

    /// Hand back the underlying map. Used by `Config::current()` to
    /// seed `.blog-os.toml`'s `[classifications.*]` tables.
    pub fn into_btreemap(self) -> BTreeMap<String, Dimension> {
        self.dimensions
    }

    /// Same as `into_btreemap` but doesn't consume `self`.
    pub fn to_btreemap(&self) -> BTreeMap<String, Dimension> {
        self.dimensions.clone()
    }
}

impl From<BTreeMap<String, Dimension>> for Taxonomy {
    fn from(dimensions: BTreeMap<String, Dimension>) -> Self {
        Self::new(dimensions)
    }
}

/// One bad classification value found during validation. `allowed`
/// carries the dimension's declared list so the error message
/// doubles as a cheat sheet — the user shouldn't have to grep the
/// taxonomy file to learn the legal options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub dimension: String,
    pub value: String,
    pub allowed: Vec<String>,
}

/// The dimension names `Classifications` knows about. Kept in code
/// because the struct's field set is fixed; only the *values* inside
/// each dimension are config-driven.
pub const SINGLE_VALUED: &[&str] = &[
    "format",
    "hook",
    "tone",
    "call_to_action",
    "visual_type",
    "complexity",
    "vulnerability",
    "outcome_prediction",
];
pub const MULTI_VALUED: &[&str] = &["audience", "topic", "narrative_structure", "motifs"];

fn starter_v1() -> BTreeMap<String, Dimension> {
    let mut m = BTreeMap::new();
    m.insert(
        "format".into(),
        Dimension {
            multi: false,
            required_by: None,
            values: strs(&[
                "parable",
                "thesis",
                "essay",
                "observation",
                "personal-reflection",
                "framework",
            ]),
        },
    );
    m.insert(
        "hook".into(),
        Dimension {
            multi: false,
            required_by: None,
            values: strs(&[
                "proverb",
                "contradiction",
                "direct-claim",
                "story-title",
                "question",
                "analogy",
            ]),
        },
    );
    m.insert(
        "tone".into(),
        Dimension {
            multi: false,
            required_by: None,
            values: strs(&["gentle", "sharp", "vulnerable", "reflective", "provocative"]),
        },
    );
    m.insert(
        "audience".into(),
        Dimension {
            multi: true,
            required_by: None,
            values: strs(&[
                "engineering",
                "product",
                "leadership",
                "founders",
                "general",
            ]),
        },
    );
    m.insert(
        "topic".into(),
        Dimension {
            multi: true,
            required_by: None,
            values: strs(&["engineering", "leadership", "product", "career", "general"]),
        },
    );
    m.insert(
        "narrative_structure".into(),
        Dimension {
            multi: true,
            required_by: None,
            values: strs(&[
                "direct-statement",
                "personal-anecdote",
                "metaphor",
                "analogy",
                "contrast",
            ]),
        },
    );
    m.insert(
        "call_to_action".into(),
        Dimension {
            multi: false,
            required_by: None,
            values: strs(&["none", "reflection", "discussion"]),
        },
    );
    m.insert(
        "visual_type".into(),
        Dimension {
            multi: false,
            required_by: None,
            values: strs(&["text-only", "diagram", "illustration", "screenshot"]),
        },
    );
    m.insert(
        "complexity".into(),
        Dimension {
            multi: false,
            required_by: None,
            values: strs(&["simple", "moderate", "dense"]),
        },
    );
    m.insert(
        "vulnerability".into(),
        Dimension {
            multi: false,
            required_by: None,
            values: strs(&["none", "low", "medium", "high"]),
        },
    );
    m.insert(
        "outcome_prediction".into(),
        Dimension {
            multi: false,
            required_by: None,
            values: strs(&["low", "medium", "high"]),
        },
    );
    m.insert(
        "motifs".into(),
        Dimension {
            multi: true,
            required_by: None,
            values: strs(&[
                "ambiguity",
                "delivery",
                "interfaces",
                "leadership",
                "ai",
                "engineering-culture",
                "product",
                "organizational-psychology",
                // Cross-disciplinary values that show up in the
                // blogs-and-posts taxonomy too — kept short, full
                // vocabulary lives in `.blog-os.toml`.
                "adaptation",
                "tradeoffs",
                "learning",
                "community",
            ]),
        },
    );
    m
}

fn strs(slice: &[&str]) -> Vec<String> {
    slice.iter().map(|s| (*s).to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_v1_includes_every_known_dimension() {
        let t = Taxonomy::current_v1();
        for name in SINGLE_VALUED.iter().chain(MULTI_VALUED.iter()) {
            assert!(
                t.dimension(name).is_some(),
                "current_v1 should declare {name}",
            );
        }
    }

    #[test]
    fn current_v1_motifs_is_multi_valued() {
        let t = Taxonomy::current_v1();
        let motifs = t.dimension("motifs").unwrap();
        assert!(motifs.multi);
    }

    #[test]
    fn current_v1_format_is_single_valued() {
        let t = Taxonomy::current_v1();
        let format = t.dimension("format").unwrap();
        assert!(!format.multi);
        assert!(format.allows("thesis"));
        assert!(!format.allows("thesys"));
    }

    #[test]
    fn empty_taxonomy_is_default() {
        assert!(Taxonomy::default().is_empty());
    }

    #[test]
    fn dimension_from_toml_round_trips() {
        let raw = "multi = true\nvalues = [\"a\", \"b\"]\n";
        let d: Dimension = toml::from_str(raw).unwrap();
        assert!(d.multi);
        assert_eq!(d.values.len(), 2);
        assert!(d.allows("a"));
        assert!(!d.allows("c"));
    }

    #[test]
    fn dimension_multi_defaults_to_false() {
        let raw = "values = [\"x\", \"y\"]\n";
        let d: Dimension = toml::from_str(raw).unwrap();
        assert!(!d.multi);
    }

    #[test]
    fn dimension_required_by_defaults_to_none() {
        let raw = "values = [\"x\"]\n";
        let d: Dimension = toml::from_str(raw).unwrap();
        assert!(d.required_by.is_none());
    }

    #[test]
    fn dimension_required_by_parses_each_stage() {
        for stage_str in [
            "concept",
            "ideation",
            "editing",
            "final-editing",
            "published",
        ] {
            let raw = format!("values = [\"x\"]\nrequired_by = \"{stage_str}\"\n");
            let d: Dimension = toml::from_str(&raw).unwrap();
            assert!(
                d.required_by.is_some(),
                "expected required_by = {stage_str:?} to parse",
            );
        }
    }

    #[test]
    fn dimension_required_by_rejects_unknown_stage() {
        let raw = "values = [\"x\"]\nrequired_by = \"draft\"\n";
        assert!(toml::from_str::<Dimension>(raw).is_err());
    }

    #[test]
    fn dimension_required_by_round_trips_through_toml() {
        let d = Dimension {
            multi: false,
            values: vec!["x".into()],
            required_by: Some(Stage::Published),
        };
        let raw = toml::to_string(&d).unwrap();
        let back: Dimension = toml::from_str(&raw).unwrap();
        assert_eq!(back, d);
    }
}

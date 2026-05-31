//! Structured tag dimensions for analytics. Sits alongside the
//! free-form `tags: Vec<String>` on `PostMetadata`: each named field
//! captures one dimension of the post's intent (format, hook, tone,
//! audience, topic, narrative-structure, call-to-action, visual-type,
//! complexity, vulnerability, outcome-prediction) plus a multi-valued
//! `motifs` list.
//!
//! Values stay `Option<String>` / `Vec<String>` rather than enums so
//! the taxonomy can evolve in `.blog-os.toml` without touching code.
//! Validation against the declared taxonomy is a separate pass (see
//! the `classify` command + workdir-config taxonomy issue) — this
//! module is the storage shape only.

use serde::{Deserialize, Deserializer, Serialize};

use crate::taxonomy::{Taxonomy, Violation};

/// One classification per dimension. Every field is optional or
/// defaulted so older posts (predating this field) parse transparently
/// via `#[serde(default)]` on `PostMetadata::classifications`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Classifications {
    /// What kind of writing this is — e.g. `parable`, `essay`,
    /// `framework`. Single-valued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,

    /// The opening move — what gets a reader in. Single-valued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook: Option<String>,

    /// Emotional register — e.g. `reflective`, `playful`, `cautionary`.
    /// Single-valued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<String>,

    /// Who the post is most likely to engage. Multi-valued (a post can
    /// land for more than one audience). Accepts a single string on
    /// the input side too, transparently promoted to a one-element
    /// list — keeps older posts from breaking when this field was
    /// `Option<String>`.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "string_or_vec"
    )]
    pub audience: Vec<String>,

    /// What the post is about — e.g. `ai`, `leadership`, `engineering`.
    /// Multi-valued.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topic: Vec<String>,

    /// How the idea is delivered — e.g. `direct-statement`,
    /// `animal-parable`, `venn-diagram`. Multi-valued so a piece can
    /// combine devices.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub narrative_structure: Vec<String>,

    /// What the reader is invited to do — e.g. `reflection`,
    /// `discussion`, `attend-event`. Single-valued; `none` is a real
    /// option.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_to_action: Option<String>,

    /// Dominant visual artifact — e.g. `text-only`, `diagram`,
    /// `carousel`. Single-valued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_type: Option<String>,

    /// Cognitive load — `simple`, `moderate`, `dense`. Single-valued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity: Option<String>,

    /// How much of the author is in the piece — `none`, `low`,
    /// `medium`, `high`. Single-valued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vulnerability: Option<String>,

    /// Pre-publish engagement guess — `low`, `medium`, `high`. Set
    /// before publishing to compare against actual performance later.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_prediction: Option<String>,

    /// Recurring conceptual threads — e.g. `adaptation`, `tradeoffs`,
    /// `community`. Multi-valued. Previously named `theme`; renamed to
    /// disambiguate from the singular narrative `theme` field on
    /// `PostMetadata` (which picks a `[themes.*]` registry entry).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub motifs: Vec<String>,
}

impl Classifications {
    /// True when every dimension is unset. Used by the post renderer's
    /// `skip_serializing_if` to keep frontmatter quiet for posts that
    /// haven't been classified yet.
    pub fn is_empty(&self) -> bool {
        self.format.is_none()
            && self.hook.is_none()
            && self.tone.is_none()
            && self.audience.is_empty()
            && self.topic.is_empty()
            && self.narrative_structure.is_empty()
            && self.call_to_action.is_none()
            && self.visual_type.is_none()
            && self.complexity.is_none()
            && self.vulnerability.is_none()
            && self.outcome_prediction.is_none()
            && self.motifs.is_empty()
    }

    /// Enumerate every value not allowed by `taxonomy`. Dimensions
    /// the taxonomy doesn't declare are skipped silently — that lets
    /// the user phase a dimension in or out of `.blog-os.toml`
    /// without invalidating posts in the meantime.
    ///
    /// Returns every violation in one pass (doctor consumes the
    /// full list); fail-fast callers wrap this in
    /// `validate(&Taxonomy)`.
    pub fn violations(&self, taxonomy: &Taxonomy) -> Vec<Violation> {
        let mut out = Vec::new();
        for (name, opt) in self.single_valued_entries() {
            let Some(value) = opt else { continue };
            let Some(dim) = taxonomy.dimension(name) else {
                continue;
            };
            if !dim.allows(value) {
                out.push(Violation {
                    dimension: name.to_string(),
                    value: value.to_string(),
                    allowed: dim.values.clone(),
                });
            }
        }
        for (name, values) in self.multi_valued_entries() {
            let Some(dim) = taxonomy.dimension(name) else {
                continue;
            };
            for value in values {
                if !dim.allows(value) {
                    out.push(Violation {
                        dimension: name.to_string(),
                        value: value.clone(),
                        allowed: dim.values.clone(),
                    });
                }
            }
        }
        out
    }

    /// Convenience: `Ok(())` when every classified value is allowed,
    /// `Err` on the first violation. Repository load paths use this;
    /// doctor uses `violations()` directly to enumerate.
    pub fn validate(&self, taxonomy: &Taxonomy) -> Result<(), Violation> {
        match self.violations(taxonomy).into_iter().next() {
            Some(v) => Err(v),
            None => Ok(()),
        }
    }

    /// Names of dimensions that are required at `stage` (per each
    /// declared dimension's `required_by`) but unset on this post.
    /// "Unset" means `None` for single-valued, empty for multi-valued.
    ///
    /// `Abandoned` posts always come back empty — they bypass the
    /// completeness check entirely; see
    /// `Stage::triggers_required_by`. Dimensions the taxonomy doesn't
    /// declare are silent (consistent with `violations`).
    pub fn missing_at_stage(
        &self,
        taxonomy: &Taxonomy,
        stage: crate::stage::Stage,
    ) -> Vec<&'static str> {
        let mut out = Vec::new();
        for (name, opt) in self.single_valued_entries() {
            let Some(dim) = taxonomy.dimension(name) else {
                continue;
            };
            let Some(threshold) = dim.required_by else {
                continue;
            };
            if stage.triggers_required_by(threshold) && opt.is_none() {
                out.push(name);
            }
        }
        for (name, values) in self.multi_valued_entries() {
            let Some(dim) = taxonomy.dimension(name) else {
                continue;
            };
            let Some(threshold) = dim.required_by else {
                continue;
            };
            if stage.triggers_required_by(threshold) && values.is_empty() {
                out.push(name);
            }
        }
        out
    }

    fn single_valued_entries(&self) -> [(&'static str, Option<&str>); 8] {
        [
            ("format", self.format.as_deref()),
            ("hook", self.hook.as_deref()),
            ("tone", self.tone.as_deref()),
            ("call_to_action", self.call_to_action.as_deref()),
            ("visual_type", self.visual_type.as_deref()),
            ("complexity", self.complexity.as_deref()),
            ("vulnerability", self.vulnerability.as_deref()),
            ("outcome_prediction", self.outcome_prediction.as_deref()),
        ]
    }

    fn multi_valued_entries(&self) -> [(&'static str, &[String]); 4] {
        [
            ("audience", &self.audience),
            ("topic", &self.topic),
            ("narrative_structure", &self.narrative_structure),
            ("motifs", &self.motifs),
        ]
    }
}

/// Deserialize a field that may be a single string OR a list of
/// strings into `Vec<String>`. Lets `audience: engineering` and
/// `audience: [engineering]` both parse — kept for forward
/// compatibility with posts written before `audience` was promoted
/// to multi-valued.
fn string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Repr {
        One(String),
        Many(Vec<String>),
    }
    match Repr::deserialize(deserializer)? {
        Repr::One(s) => Ok(vec![s]),
        Repr::Many(v) => Ok(v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full() -> Classifications {
        Classifications {
            format: Some("thesis".into()),
            hook: Some("contradiction".into()),
            tone: Some("sharp".into()),
            audience: vec!["engineering".into()],
            topic: vec!["leadership".into()],
            narrative_structure: vec!["analogy".into()],
            call_to_action: Some("reflection".into()),
            visual_type: Some("text-only".into()),
            complexity: Some("moderate".into()),
            vulnerability: Some("low".into()),
            outcome_prediction: Some("medium".into()),
            motifs: vec!["ambiguity".into(), "delivery".into()],
        }
    }

    #[test]
    fn default_is_empty() {
        assert!(Classifications::default().is_empty());
    }

    #[test]
    fn full_round_trips_through_yaml() {
        let c = full();
        let yaml = serde_yaml_ng::to_string(&c).unwrap();
        let back: Classifications = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn empty_dimensions_are_skipped_in_output() {
        // A partially populated classification — only `format` set —
        // should serialize to a single line, not a block of nulls.
        let c = Classifications {
            format: Some("thesis".into()),
            ..Default::default()
        };
        let yaml = serde_yaml_ng::to_string(&c).unwrap();
        assert!(yaml.contains("format: thesis"), "got: {yaml}");
        assert!(!yaml.contains("hook"), "hook must be skipped: {yaml}");
        assert!(!yaml.contains("tone"), "tone must be skipped: {yaml}");
        assert!(!yaml.contains("motifs"), "motifs must be skipped: {yaml}");
        assert!(!yaml.contains("topic"), "topic must be skipped: {yaml}");
        assert!(
            !yaml.contains("visual_type"),
            "visual_type must be skipped: {yaml}",
        );
    }

    #[test]
    fn missing_fields_parse_as_defaults() {
        // Pre-classifications-era YAML: just a single dimension.
        let raw = "format: thesis\n";
        let c: Classifications = serde_yaml_ng::from_str(raw).unwrap();
        assert_eq!(c.format.as_deref(), Some("thesis"));
        assert!(c.hook.is_none());
        assert!(c.motifs.is_empty());
        assert!(c.topic.is_empty());
        assert!(c.call_to_action.is_none());
    }

    #[test]
    fn empty_yaml_object_parses_as_default() {
        // A `classifications: {}` block in the parent document — every
        // field defaulted out, equal to `Classifications::default()`.
        let c: Classifications = serde_yaml_ng::from_str("{}").unwrap();
        assert_eq!(c, Classifications::default());
        assert!(c.is_empty());
    }

    #[test]
    fn motifs_round_trips_multi_value() {
        let c = Classifications {
            motifs: vec!["ambiguity".into(), "delivery".into(), "interfaces".into()],
            ..Default::default()
        };
        let yaml = serde_yaml_ng::to_string(&c).unwrap();
        let back: Classifications = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(back, c);
        assert_eq!(back.motifs.len(), 3);
    }

    #[test]
    fn motifs_single_element_round_trips() {
        // The common case: one motif, written as a single-element list.
        let c = Classifications {
            motifs: vec!["ambiguity".into()],
            ..Default::default()
        };
        let yaml = serde_yaml_ng::to_string(&c).unwrap();
        let back: Classifications = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn audience_accepts_single_string_for_back_compat() {
        // Pre-promotion frontmatter: `audience: engineering` (string).
        // Must parse cleanly into a one-element Vec so old posts don't
        // break when `audience` flipped from Option<String> to Vec<String>.
        let raw = "audience: engineering\n";
        let c: Classifications = serde_yaml_ng::from_str(raw).unwrap();
        assert_eq!(c.audience, vec!["engineering"]);
    }

    #[test]
    fn audience_accepts_list_form() {
        let raw = "audience:\n  - engineering\n  - leadership\n";
        let c: Classifications = serde_yaml_ng::from_str(raw).unwrap();
        assert_eq!(c.audience, vec!["engineering", "leadership"]);
    }

    #[test]
    fn audience_round_trips_as_list() {
        let c = Classifications {
            audience: vec!["engineering".into(), "founders".into()],
            ..Default::default()
        };
        let yaml = serde_yaml_ng::to_string(&c).unwrap();
        let back: Classifications = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn validate_accepts_every_v1_value() {
        let c = full();
        let t = Taxonomy::current_v1();
        assert_eq!(c.violations(&t), vec![]);
        assert!(c.validate(&t).is_ok());
    }

    #[test]
    fn validate_rejects_typo_in_single_valued_dimension() {
        let c = Classifications {
            format: Some("thesys".into()),
            ..Default::default()
        };
        let t = Taxonomy::current_v1();
        let err = c.validate(&t).expect_err("should reject typo");
        assert_eq!(err.dimension, "format");
        assert_eq!(err.value, "thesys");
        // Error carries the allowed list — it's its own cheat sheet.
        assert!(err.allowed.contains(&"thesis".to_string()));
    }

    #[test]
    fn validate_rejects_unknown_motif_element() {
        // Only the bad element shows up — the good one is silent.
        let c = Classifications {
            motifs: vec!["ambiguity".into(), "made-up".into()],
            ..Default::default()
        };
        let v = c.violations(&Taxonomy::current_v1());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].dimension, "motifs");
        assert_eq!(v[0].value, "made-up");
    }

    #[test]
    fn violations_lists_every_bad_value_in_one_pass() {
        let c = Classifications {
            format: Some("madeup-format".into()),
            tone: Some("madeup-tone".into()),
            motifs: vec!["madeup-motif".into()],
            ..Default::default()
        };
        let v = c.violations(&Taxonomy::current_v1());
        assert_eq!(v.len(), 3);
        let dims: Vec<&str> = v.iter().map(|x| x.dimension.as_str()).collect();
        assert!(dims.contains(&"format"));
        assert!(dims.contains(&"tone"));
        assert!(dims.contains(&"motifs"));
    }

    #[test]
    fn validate_rejects_unknown_topic_and_audience_elements() {
        let c = Classifications {
            topic: vec!["leadership".into(), "made-up-topic".into()],
            audience: vec!["engineering".into(), "made-up-audience".into()],
            ..Default::default()
        };
        let v = c.violations(&Taxonomy::current_v1());
        let dims: Vec<&str> = v.iter().map(|x| x.dimension.as_str()).collect();
        assert!(dims.contains(&"topic"));
        assert!(dims.contains(&"audience"));
    }

    #[test]
    fn validate_silent_on_dimensions_taxonomy_does_not_declare() {
        // Empty taxonomy — nothing to validate against, nothing to reject.
        let c = full();
        let t = Taxonomy::default();
        assert!(c.violations(&t).is_empty());
        assert!(c.validate(&t).is_ok());
    }

    #[test]
    fn validate_accepts_unset_dimensions_even_with_taxonomy() {
        // A post that hasn't been classified yet — every dim is None —
        // must validate cleanly against any taxonomy.
        let c = Classifications::default();
        let t = Taxonomy::current_v1();
        assert!(c.validate(&t).is_ok());
    }

    use crate::stage::Stage;
    use crate::taxonomy::Dimension;
    use std::collections::BTreeMap;

    fn taxonomy_with(name: &str, dim: Dimension) -> Taxonomy {
        let mut m = BTreeMap::new();
        m.insert(name.to_string(), dim);
        Taxonomy::new(m)
    }

    #[test]
    fn missing_at_stage_flags_unset_required_single_dim() {
        let t = taxonomy_with(
            "format",
            Dimension {
                values: vec!["essay".into(), "parable".into()],
                required_by: Some(Stage::Published),
                ..Default::default()
            },
        );
        let c = Classifications::default();
        assert_eq!(
            c.missing_at_stage(&t, Stage::Published),
            vec!["format"],
            "published post with missing required format should be flagged",
        );
    }

    #[test]
    fn missing_at_stage_silent_when_post_has_not_reached_threshold() {
        let t = taxonomy_with(
            "format",
            Dimension {
                values: vec!["essay".into()],
                required_by: Some(Stage::Published),
                ..Default::default()
            },
        );
        let c = Classifications::default();
        // Earlier stages skip — required-ness only activates at the
        // threshold.
        for &stage in &[
            Stage::Concept,
            Stage::Ideation,
            Stage::Editing,
            Stage::FinalEditing,
        ] {
            assert!(
                c.missing_at_stage(&t, stage).is_empty(),
                "stage {stage} should bypass `required_by = published`",
            );
        }
    }

    #[test]
    fn missing_at_stage_silent_when_dim_is_set() {
        let t = taxonomy_with(
            "format",
            Dimension {
                values: vec!["essay".into()],
                required_by: Some(Stage::Published),
                ..Default::default()
            },
        );
        let c = Classifications {
            format: Some("essay".into()),
            ..Default::default()
        };
        assert!(c.missing_at_stage(&t, Stage::Published).is_empty());
    }

    #[test]
    fn missing_at_stage_silent_for_abandoned_posts() {
        // Abandoned posts always bypass — the check never fires
        // regardless of `required_by`.
        let t = taxonomy_with(
            "format",
            Dimension {
                values: vec!["essay".into()],
                required_by: Some(Stage::Published),
                ..Default::default()
            },
        );
        let c = Classifications::default();
        assert!(c.missing_at_stage(&t, Stage::Abandoned).is_empty());
    }

    #[test]
    fn missing_at_stage_flags_empty_required_multi_dim() {
        let t = taxonomy_with(
            "audience",
            Dimension {
                multi: true,
                values: vec!["engineering".into(), "general".into()],
                required_by: Some(Stage::Published),
            },
        );
        let c = Classifications::default(); // audience: Vec is empty
        assert_eq!(c.missing_at_stage(&t, Stage::Published), vec!["audience"],);
    }

    #[test]
    fn missing_at_stage_silent_when_multi_dim_has_at_least_one_value() {
        let t = taxonomy_with(
            "audience",
            Dimension {
                multi: true,
                values: vec!["engineering".into(), "general".into()],
                required_by: Some(Stage::Published),
            },
        );
        let c = Classifications {
            audience: vec!["engineering".into()],
            ..Default::default()
        };
        assert!(c.missing_at_stage(&t, Stage::Published).is_empty());
    }

    #[test]
    fn missing_at_stage_silent_on_dimensions_without_required_by() {
        // Without `required_by`, the dimension is always optional —
        // even for published posts.
        let t = taxonomy_with(
            "format",
            Dimension {
                values: vec!["essay".into()],
                required_by: None,
                ..Default::default()
            },
        );
        let c = Classifications::default();
        assert!(c.missing_at_stage(&t, Stage::Published).is_empty());
    }

    #[test]
    fn missing_at_stage_required_by_abandoned_is_no_op() {
        // Declaring `required_by = "abandoned"` never triggers — no
        // post ever needs to "reach abandoned" in pipeline order.
        let t = taxonomy_with(
            "format",
            Dimension {
                values: vec!["essay".into()],
                required_by: Some(Stage::Abandoned),
                ..Default::default()
            },
        );
        let c = Classifications::default();
        for &stage in Stage::ALL {
            assert!(c.missing_at_stage(&t, stage).is_empty());
        }
    }
}

//! Structured tag dimensions for analytics. Sits alongside the
//! free-form `tags: Vec<String>` on `PostMetadata`: each named field
//! captures one dimension of the post's intent (format, hook, tone,
//! audience) plus a multi-valued `theme` list.
//!
//! Values stay `Option<String>` / `Vec<String>` rather than enums so
//! the taxonomy can evolve in `.blog-os.toml` without touching code.
//! Validation against the declared taxonomy is a separate pass (see
//! the `classify` command + workdir-config taxonomy issue) — this
//! module is the storage shape only.

use serde::{Deserialize, Serialize};

use crate::taxonomy::{Taxonomy, Violation};

/// One classification per dimension. Every field is optional or
/// defaulted so older posts (predating this field) parse transparently
/// via `#[serde(default)]` on `PostMetadata::classifications`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Classifications {
    /// `parable`, `thesis`, `essay`, `observation`, `personal-reflection`,
    /// `framework`. Single-valued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,

    /// `proverb`, `contradiction`, `direct-claim`, `story-title`,
    /// `question`, `analogy`. Single-valued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook: Option<String>,

    /// `gentle`, `sharp`, `vulnerable`, `reflective`, `provocative`.
    /// Single-valued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<String>,

    /// `engineering`, `product`, `leadership`, `founders`, `general`.
    /// Single-valued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audience: Option<String>,

    /// `ambiguity`, `delivery`, `interfaces`, `leadership`, `ai`,
    /// `engineering-culture`, `product`, `organizational-psychology`.
    /// Multi-valued — a post can sit in several themes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub theme: Vec<String>,
}

impl Classifications {
    /// True when every dimension is unset. Used by the post renderer's
    /// `skip_serializing_if` to keep frontmatter quiet for posts that
    /// haven't been classified yet.
    pub fn is_empty(&self) -> bool {
        self.format.is_none()
            && self.hook.is_none()
            && self.tone.is_none()
            && self.audience.is_none()
            && self.theme.is_empty()
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

    fn single_valued_entries(&self) -> [(&'static str, Option<&str>); 4] {
        [
            ("format", self.format.as_deref()),
            ("hook", self.hook.as_deref()),
            ("tone", self.tone.as_deref()),
            ("audience", self.audience.as_deref()),
        ]
    }

    fn multi_valued_entries(&self) -> [(&'static str, &[String]); 1] {
        [("theme", &self.theme)]
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
            audience: Some("engineering".into()),
            theme: vec!["ambiguity".into(), "delivery".into()],
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
        assert!(!yaml.contains("theme"), "theme must be skipped: {yaml}");
    }

    #[test]
    fn missing_fields_parse_as_defaults() {
        // Pre-classifications-era YAML: just a single dimension.
        let raw = "format: thesis\n";
        let c: Classifications = serde_yaml_ng::from_str(raw).unwrap();
        assert_eq!(c.format.as_deref(), Some("thesis"));
        assert!(c.hook.is_none());
        assert!(c.theme.is_empty());
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
    fn theme_round_trips_multi_value() {
        let c = Classifications {
            theme: vec!["ambiguity".into(), "delivery".into(), "interfaces".into()],
            ..Default::default()
        };
        let yaml = serde_yaml_ng::to_string(&c).unwrap();
        let back: Classifications = serde_yaml_ng::from_str(&yaml).unwrap();
        assert_eq!(back, c);
        assert_eq!(back.theme.len(), 3);
    }

    #[test]
    fn theme_single_element_round_trips() {
        // The common case: one theme, written as a single-element list.
        let c = Classifications {
            theme: vec!["ambiguity".into()],
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
    fn validate_rejects_unknown_theme_element() {
        // Only the bad element shows up — the good one is silent.
        let c = Classifications {
            theme: vec!["ambiguity".into(), "made-up".into()],
            ..Default::default()
        };
        let v = c.violations(&Taxonomy::current_v1());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].dimension, "theme");
        assert_eq!(v[0].value, "made-up");
    }

    #[test]
    fn violations_lists_every_bad_value_in_one_pass() {
        let c = Classifications {
            format: Some("madeup-format".into()),
            tone: Some("madeup-tone".into()),
            theme: vec!["madeup-theme".into()],
            ..Default::default()
        };
        let v = c.violations(&Taxonomy::current_v1());
        assert_eq!(v.len(), 3);
        let dims: Vec<&str> = v.iter().map(|x| x.dimension.as_str()).collect();
        assert!(dims.contains(&"format"));
        assert!(dims.contains(&"tone"));
        assert!(dims.contains(&"theme"));
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
}

//! Structured tag dimensions for analytics. Sits alongside the
//! free-form `tags: Vec<String>` on `PostMetadata`: each named field
//! captures one dimension of the post's intent (format, hook, tone,
//! audience, strategic role) plus a multi-valued `theme` list.
//!
//! Values stay `Option<String>` / `Vec<String>` rather than enums so
//! the taxonomy can evolve in `.blog-os.toml` without touching code.
//! Validation against the declared taxonomy is a separate pass (see
//! the `classify` command + workdir-config taxonomy issue) — this
//! module is the storage shape only.

use serde::{Deserialize, Serialize};

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

    /// `salal-positioning`, `career-brand`, `recruiting`,
    /// `writing-practice`, `consulting-signal`. Single-valued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategic_role: Option<String>,

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
            && self.strategic_role.is_none()
            && self.theme.is_empty()
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
            strategic_role: Some("career-brand".into()),
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
}

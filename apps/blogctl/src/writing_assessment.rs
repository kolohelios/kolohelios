//! Deterministic mechanical checks, never editorial approval or selection.

use serde::{Deserialize, Serialize};

use crate::writing::CandidateDocument;
use crate::writing_workflow::WorkflowPass;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum AssessmentStatus {
    NeedsReview,
    ConstraintViolation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub code: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assessment {
    pub status: AssessmentStatus,
    pub words: usize,
    pub findings: Vec<Finding>,
}

pub fn assess(pass: &WorkflowPass, candidate: &CandidateDocument) -> Assessment {
    let body = &candidate.body;
    let words = body
        .split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '’')
        .filter(|word| !word.is_empty())
        .count();
    let mut findings = Vec::new();
    let mut finding = |code: &str, detail: String| {
        findings.push(Finding {
            code: code.into(),
            detail,
        })
    };
    if pass.min_words().is_some_and(|minimum| words < minimum) {
        finding(
            "below-minimum-words",
            format!("{words} words is below the pass minimum"),
        );
    }
    if pass.max_words().is_some_and(|maximum| words > maximum) {
        finding(
            "above-maximum-words",
            format!("{words} words exceeds the pass maximum"),
        );
    }
    let lowercase = body.to_lowercase();
    if lowercase.contains("<think") || lowercase.contains("</think") {
        finding("reasoning-markup", "candidate contains a think tag".into());
    }
    if body.contains("<|") {
        finding(
            "protocol-marker",
            "candidate contains a protocol marker".into(),
        );
    }
    if body.trim_end().ends_with('}') {
        finding(
            "instruction-brace",
            "candidate ends with a likely leaked instruct brace".into(),
        );
    }
    if body
        .lines()
        .any(|line| line.starts_with("Approach (") || line == "Candidate:")
    {
        finding(
            "instruction-scaffolding",
            "candidate contains editorial scaffolding".into(),
        );
    }
    if pass.forbid_workflow_metadata()
        && (lowercase.contains("next action:") || lowercase.contains("status:"))
    {
        finding(
            "workflow-metadata",
            "candidate contains likely invented workflow metadata".into(),
        );
    }
    if pass.require_complete_ending()
        && !matches!(
            body.trim_end().chars().last(),
            Some('.' | '!' | '?' | ')' | ']' | '"' | '\'')
        )
    {
        finding(
            "incomplete-ending",
            "candidate appears to end mid-sentence".into(),
        );
    }
    for (index, protected) in pass.protected_text().iter().enumerate() {
        if !body.contains(protected) {
            finding(
                "protected-text-missing",
                format!("protected text item {} was changed or removed", index + 1),
            );
        }
    }
    for (index, forbidden) in pass.forbidden_strings().iter().enumerate() {
        if lowercase.contains(&forbidden.to_lowercase()) {
            finding(
                "forbidden-output",
                format!("forbidden output item {} is present", index + 1),
            );
        }
    }
    if candidate.provenance.provider == "novelai" {
        if candidate
            .provenance
            .parameters
            .get("finish_reason")
            .and_then(serde_json::Value::as_str)
            != Some("stop")
        {
            finding(
                "completion-not-established",
                "provider did not report a normal stop".into(),
            );
        }
        if candidate
            .provenance
            .parameters
            .get("reasoning_content")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|text| !text.is_empty())
        {
            finding(
                "provider-reasoning",
                "provider returned separate reasoning".into(),
            );
        }
    }
    Assessment {
        status: if findings.is_empty() {
            AssessmentStatus::NeedsReview
        } else {
            AssessmentStatus::ConstraintViolation
        },
        words,
        findings,
    }
}

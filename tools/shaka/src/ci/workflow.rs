use std::collections::BTreeMap;

use serde::Serialize;
use serde_yaml_ng::Value;

#[derive(Serialize, Debug)]
pub struct Workflow {
    pub name: String,
    pub on: On,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Permissions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<Concurrency>,
    pub jobs: BTreeMap<String, Job>,
}

#[derive(Serialize, Debug, Default)]
pub struct On {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<Empty>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push: Option<PushTrigger>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_dispatch: Option<Empty>,
}

#[derive(Serialize, Debug)]
pub struct PushTrigger {
    pub branches: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
}

#[derive(Debug)]
pub struct Empty;

impl Serialize for Empty {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_none()
    }
}

#[derive(Serialize, Debug)]
pub struct Permissions {
    #[serde(rename = "id-token", skip_serializing_if = "Option::is_none")]
    pub id_token: Option<PermissionLevel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contents: Option<PermissionLevel>,
    #[serde(rename = "pull-requests", skip_serializing_if = "Option::is_none")]
    pub pull_requests: Option<PermissionLevel>,
}

#[derive(Serialize, Debug, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum PermissionLevel {
    Read,
    Write,
}

/// `cancel-in-progress` may be a literal bool or a templated GitHub
/// expression (`${{ ... }}`). Modeled as a string so callers can emit
/// either; YAML parses both forms as the same field value.
#[derive(Serialize, Debug)]
pub struct Concurrency {
    pub group: String,
    #[serde(rename = "cancel-in-progress")]
    pub cancel_in_progress: String,
}

#[derive(Serialize, Debug)]
#[serde(untagged)]
#[allow(dead_code)] // Inline jobs are first consumed by the main.yaml emitter (next commit).
pub enum Job {
    ReusableCall(ReusableCall),
    Inline(InlineJob),
}

#[derive(Serialize, Debug)]
pub struct ReusableCall {
    pub uses: String,
    pub with: BTreeMap<String, String>,
    pub secrets: BTreeMap<String, String>,
}

/// Either a single job id (`needs: changes`) or a list of job ids
/// (`needs: [preflight, changes]`). Both forms appear in GitHub
/// Actions; serde_yaml_ng emits them as the matching YAML shape.
#[derive(Serialize, Debug)]
#[serde(untagged)]
#[allow(dead_code)] // First consumer is the main.yaml emitter (next commit).
pub enum Needs {
    Single(String),
    Multiple(Vec<String>),
}

impl Needs {
    pub fn is_empty(&self) -> bool {
        match self {
            Needs::Single(s) => s.is_empty(),
            Needs::Multiple(v) => v.is_empty(),
        }
    }
}

#[derive(Serialize, Debug)]
#[allow(dead_code)] // First consumer is the main.yaml emitter (next commit).
pub struct InlineJob {
    pub name: String,
    #[serde(skip_serializing_if = "Needs::is_empty")]
    pub needs: Needs,
    #[serde(rename = "if", skip_serializing_if = "Option::is_none")]
    pub if_: Option<String>,
    #[serde(rename = "runs-on")]
    pub runs_on: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Permissions>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub outputs: BTreeMap<String, String>,
    pub steps: Vec<Step>,
}

/// A step is either an action invocation (`uses: org/action@ref` with
/// optional `with:` inputs) or a shell command (`run: <script>` with
/// optional `name:`). Steps may carry an `if:` guard and an `env:`
/// block in either form.
#[derive(Serialize, Debug)]
#[serde(untagged)]
#[allow(dead_code)] // First consumer is the main.yaml emitter (next commit).
pub enum Step {
    Action(ActionStep),
    Run(RunStep),
}

#[derive(Serialize, Debug)]
#[allow(dead_code)] // First consumer is the main.yaml emitter (next commit).
pub struct ActionStep {
    pub uses: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "if", skip_serializing_if = "Option::is_none")]
    pub if_: Option<String>,
    /// Step inputs. Values are arbitrary YAML — strings, bools,
    /// numbers — because GitHub action inputs differ by action.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub with: BTreeMap<String, Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

#[derive(Serialize, Debug)]
#[allow(dead_code)] // First consumer is the main.yaml emitter (next commit).
pub struct RunStep {
    pub name: String,
    #[serde(rename = "if", skip_serializing_if = "Option::is_none")]
    pub if_: Option<String>,
    pub run: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

const HEADER: &str = "# Generated by shaka. Do not edit by hand.\n# Run `shaka ci generate-workflows` to refresh.\n#\n# Source of truth: <slot>/<project>/project.cue.\n\n";

/// Serialize a workflow to YAML with the canonical "do not edit" header.
pub fn emit(workflow: &Workflow) -> String {
    let body = serde_yaml_ng::to_string(workflow).expect("workflow serializes");
    format!("{HEADER}{body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_apply_workflow(name: &str, project_dir: &str) -> Workflow {
        Workflow {
            name: format!("{name} apply"),
            on: On {
                pull_request: None,
                push: Some(PushTrigger {
                    branches: vec!["main".into()],
                    paths: vec![
                        format!("{project_dir}/**"),
                        format!(".github/workflows/{name}-apply.yml"),
                        ".github/workflows/tf-apply.yml".into(),
                    ],
                }),
                workflow_dispatch: Some(Empty),
            },
            permissions: Some(Permissions {
                id_token: Some(PermissionLevel::Write),
                contents: Some(PermissionLevel::Read),
                pull_requests: None,
            }),
            concurrency: Some(Concurrency {
                group: format!("{name}-apply"),
                cancel_in_progress: "false".to_string(),
            }),
            jobs: {
                let mut m = BTreeMap::new();
                m.insert(
                    "apply".to_string(),
                    Job::ReusableCall(ReusableCall {
                        uses: "./.github/workflows/tf-apply.yml".into(),
                        with: {
                            let mut w = BTreeMap::new();
                            w.insert("project_dir".to_string(), project_dir.to_string());
                            w
                        },
                        secrets: {
                            let mut s = BTreeMap::new();
                            s.insert(
                                "OP_SERVICE_ACCOUNT_TOKEN".to_string(),
                                "${{ secrets.OP_SERVICE_ACCOUNT_TOKEN }}".to_string(),
                            );
                            s
                        },
                    }),
                );
                m
            },
        }
    }

    #[test]
    fn emit_starts_with_do_not_edit_header() {
        let yaml = emit(&fixture_apply_workflow(
            "cloudflare-dns",
            "infra/cloudflare-dns",
        ));
        assert!(yaml.starts_with("# Generated by shaka. Do not edit by hand.\n"));
    }

    #[test]
    fn emit_round_trips_to_valid_yaml() {
        let yaml = emit(&fixture_apply_workflow(
            "cloudflare-dns",
            "infra/cloudflare-dns",
        ));
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).expect("valid YAML");
        assert_eq!(parsed["name"].as_str(), Some("cloudflare-dns apply"));
        assert_eq!(
            parsed["jobs"]["apply"]["uses"].as_str(),
            Some("./.github/workflows/tf-apply.yml"),
        );
    }

    #[test]
    fn emit_is_deterministic() {
        let a = emit(&fixture_apply_workflow(
            "cloudflare-dns",
            "infra/cloudflare-dns",
        ));
        let b = emit(&fixture_apply_workflow(
            "cloudflare-dns",
            "infra/cloudflare-dns",
        ));
        assert_eq!(a, b);
    }

    #[test]
    fn inline_job_with_action_step_emits_runs_on() {
        let mut jobs = BTreeMap::new();
        jobs.insert(
            "preflight".to_string(),
            Job::Inline(InlineJob {
                name: "Preflight".to_string(),
                needs: Needs::Single("changes".to_string()),
                if_: None,
                runs_on: "ubuntu-latest".to_string(),
                permissions: None,
                env: BTreeMap::new(),
                outputs: BTreeMap::new(),
                steps: vec![Step::Action(ActionStep {
                    uses: "actions/checkout@v6".to_string(),
                    id: None,
                    name: None,
                    if_: None,
                    with: {
                        let mut w = BTreeMap::new();
                        w.insert("fetch-depth".to_string(), Value::Number(0.into()));
                        w
                    },
                    env: BTreeMap::new(),
                })],
            }),
        );
        let wf = Workflow {
            name: "CI".to_string(),
            on: On::default(),
            permissions: None,
            concurrency: None,
            jobs,
        };
        let yaml = emit(&wf);
        assert!(yaml.contains("runs-on: ubuntu-latest"));
        assert!(yaml.contains("fetch-depth: 0"));
        assert!(yaml.contains("uses: actions/checkout@v6"));
    }

    #[test]
    fn needs_serializes_as_string_or_list() {
        let single = serde_yaml_ng::to_string(&Needs::Single("changes".into())).unwrap();
        assert_eq!(single.trim(), "changes");
        let multi =
            serde_yaml_ng::to_string(&Needs::Multiple(vec!["a".into(), "b".into()])).unwrap();
        assert!(multi.contains("- a"));
        assert!(multi.contains("- b"));
    }
}

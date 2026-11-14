//! Opt-in standalone validation through the same repo-wide preflight used locally.
//! The declaring project's devshell supplies shaka; no tools/shaka checkout,
//! publishing configuration, application secrets, or path filters are needed.

use std::collections::BTreeMap;
use std::path::Path;

use indexmap::IndexMap;
use serde::Deserialize;
use serde_yaml_ng::Value;

use super::workflow::{
    ActionStep, CancelInProgress, Concurrency, Empty, InlineJob, Job, Needs, On, PermissionLevel,
    Permissions, PullRequestTrigger, PushTrigger, RunStep, Step, Workflow,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiValidate {}

pub fn build(name: &str, project_dir: &Path) -> Workflow {
    // Quote paths even though canonical slot/name paths normally need no quoting.
    let path = project_dir.to_string_lossy().replace('\'', "'\\''");
    let develop = format!("nix develop './{path}' --command");
    let action = |uses: &str, with: BTreeMap<String, Value>| {
        Step::Action(ActionStep {
            uses: uses.to_string(),
            id: None,
            name: None,
            if_: None,
            with,
            env: BTreeMap::new(),
        })
    };
    let run = |name: &str, command: String, if_: Option<String>| {
        Step::Run(RunStep {
            id: None,
            name: Some(name.to_string()),
            if_,
            run: command,
            env: BTreeMap::new(),
        })
    };
    let job = InlineJob {
        name: Some(format!("{name} validate")),
        needs: Needs::Multiple(vec![]),
        if_: None,
        runs_on: "ubuntu-latest".to_string(),
        permissions: None,
        env: BTreeMap::new(),
        outputs: BTreeMap::new(),
        steps: vec![
            action(
                "actions/checkout@v7",
                BTreeMap::from([("fetch-depth".to_string(), Value::Number(0.into()))]),
            ),
            action(
                "DeterminateSystems/nix-installer-action@main",
                BTreeMap::from([
                    ("determinate".to_string(), Value::Bool(true)),
                    ("flakehub".to_string(), Value::Bool(true)),
                ]),
            ),
            action("DeterminateSystems/flakehub-cache-action@v3", BTreeMap::new()),
            run(
                "Colocate jj on the git checkout",
                format!("{develop} jj git init --colocate"),
                None,
            ),
            run(
                "shaka commit lint",
                format!(
                    "{develop} shaka commit lint -r '${{{{ github.event.pull_request.base.sha }}}}..${{{{ github.event.pull_request.head.sha }}}}'"
                ),
                Some("github.event_name == 'pull_request'".to_string()),
            ),
            // Deliberately unscoped: policy/schema/workflow edits must be gated
            // even when no application files changed. Preflight owns all checks,
            // including generation drift and workflow accounting.
            run("shaka preflight", format!("{develop} shaka preflight"), None),
        ],
    };
    Workflow {
        name: format!("{name} validate"),
        on: On {
            pull_request: Some(PullRequestTrigger::All(Empty)),
            push: Some(PushTrigger {
                branches: vec!["main".to_string()],
                paths: vec![],
            }),
            workflow_dispatch: Some(Empty),
        },
        permissions: Some(Permissions {
            id_token: Some(PermissionLevel::Write),
            contents: Some(PermissionLevel::Read),
            pull_requests: None,
        }),
        concurrency: Some(Concurrency {
            group: format!("{name}-validate-${{{{ github.ref }}}}"),
            cancel_in_progress: CancelInProgress::Expression(
                "${{ github.event_name == 'pull_request' }}".to_string(),
            ),
        }),
        jobs: IndexMap::from([("validate".to_string(), Job::Inline(job))]),
    }
}

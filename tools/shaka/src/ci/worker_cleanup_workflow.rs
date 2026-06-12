//! Builder for `<name>-cleanup.yml` — deletes a Worker app's
//! ephemeral preview Worker when its PR closes (merged or abandoned).
//!
//! Generated as a sibling of `<name>-deploy.yml` (see
//! `worker_deploy_workflow.rs`) so every project with a `ci.deploy`
//! block automatically gets cleanup — there is no "remember to
//! hand-author the cleanup file" step to forget (#719). Both files
//! derive from the same `previewScriptPrefix`, so the deleted name
//! always matches the name the deploy workflow's `preview` job
//! created.
//!
//! Trigger: `pull_request: [closed]` — fires for both merge and
//! abandon. `wrangler delete --force || true` is idempotent: a PR
//! that closed before its first preview ever deployed just no-ops.
//! A repo-wide `shaka deploy sweep-previews` sweep (run on `push:
//! main` and a daily cron) backstops anything this per-PR job misses.

use std::collections::BTreeMap;

use indexmap::IndexMap;
use serde_yaml_ng::Value;

use super::worker_deploy_workflow::WorkerDeploySpec;
use super::workflow::{
    ActionStep, InlineJob, Job, Needs, On, PermissionLevel, Permissions, PullRequestTrigger,
    RunStep, Step, Workflow,
};

pub fn build(spec: &WorkerDeploySpec) -> Workflow {
    let mut jobs: IndexMap<String, Job> = IndexMap::new();
    jobs.insert("cleanup".to_string(), Job::Inline(cleanup_job(spec)));

    Workflow {
        name: format!("{} cleanup", spec.project_name),
        on: On {
            pull_request: Some(PullRequestTrigger::Types {
                types: vec!["closed".to_string()],
            }),
            push: None,
            workflow_dispatch: None,
        },
        // `id-token: write` lets nix-installer authenticate with
        // FlakeHub for the `kolohelios-nix` private input; the SA
        // token (job env) resolves the `op://` refs `wrangler` needs.
        permissions: Some(Permissions {
            id_token: Some(PermissionLevel::Write),
            contents: Some(PermissionLevel::Read),
            pull_requests: None,
        }),
        concurrency: None,
        jobs,
    }
}

fn cleanup_job(spec: &WorkerDeploySpec) -> InlineJob {
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    env.insert(
        "OP_SERVICE_ACCOUNT_TOKEN".to_string(),
        "${{ secrets.OP_SERVICE_ACCOUNT_TOKEN }}".to_string(),
    );

    InlineJob {
        name: None,
        needs: Needs::Multiple(vec![]),
        if_: None,
        runs_on: "ubuntu-latest".to_string(),
        permissions: None,
        env,
        outputs: BTreeMap::new(),
        steps: vec![
            // `required: true` on the consuming workflow catches a
            // missing secret; this catches an empty value (org secret
            // unset) before the downstream `op run` surfaces a cryptic
            // error.
            Step::Run(RunStep {
                id: None,
                name: Some("verify OP_SERVICE_ACCOUNT_TOKEN".to_string()),
                if_: None,
                run: r#"if [[ -z "$OP_SERVICE_ACCOUNT_TOKEN" ]]; then
  echo "::error::OP_SERVICE_ACCOUNT_TOKEN is empty; check repo/org secret"
  exit 1
fi
"#
                .to_string(),
                env: BTreeMap::new(),
            }),
            Step::Action(ActionStep {
                uses: "actions/checkout@v6".to_string(),
                id: None,
                name: None,
                if_: None,
                with: BTreeMap::new(),
                env: BTreeMap::new(),
            }),
            Step::Action(ActionStep {
                uses: "DeterminateSystems/nix-installer-action@main".to_string(),
                id: None,
                name: None,
                if_: None,
                with: {
                    let mut w = BTreeMap::new();
                    w.insert("determinate".to_string(), Value::Bool(true));
                    w.insert("flakehub".to_string(), Value::Bool(true));
                    w
                },
                env: BTreeMap::new(),
            }),
            Step::Action(ActionStep {
                uses: "DeterminateSystems/flakehub-cache-action@v3".to_string(),
                id: None,
                name: None,
                if_: None,
                with: BTreeMap::new(),
                env: BTreeMap::new(),
            }),
            // `--force` skips interactive confirmation; safe because
            // the script name is deterministically derived from the PR
            // number. `|| true` tolerates "not found" so a PR that
            // never produced a preview (closed before first deploy)
            // doesn't fail the workflow. `.env` is gitignored — copy
            // the in-repo example so `op run` finds the `op://` refs.
            Step::Run(RunStep {
                id: None,
                name: Some("wrangler delete preview Worker".to_string()),
                if_: None,
                run: format!(
                    r#"cd {dir}
cp .env.example .env
nix develop . --command op run --env-file=.env -- \
  wrangler delete \
  --name {prefix}-pr-${{{{ github.event.pull_request.number }}}} \
  --force \
  || true
"#,
                    dir = spec.project_dir.to_string_lossy(),
                    prefix = spec.deploy.preview_script_prefix,
                ),
                env: BTreeMap::new(),
            }),
        ],
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::super::worker_deploy_workflow::{CiDeploy, WorkerDeploySpec};
    use super::*;

    fn fixture_spec() -> WorkerDeploySpec {
        WorkerDeploySpec {
            project_dir: PathBuf::from("apps/example-notes"),
            project_name: "example-notes".to_string(),
            deploy: CiDeploy {
                reusable_workflow: "./.github/workflows/cf-deploy.yml".to_string(),
                preview_script_prefix: "example-notes".to_string(),
                environment: None,
            },
        }
    }

    #[test]
    fn triggers_only_on_pull_request_closed() {
        let yaml = super::super::workflow::emit(&build(&fixture_spec()));
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).expect("valid YAML");
        let types = &parsed["on"]["pull_request"]["types"];
        assert_eq!(types[0].as_str(), Some("closed"));
        assert!(types[1].is_null(), "only `closed`, no other activity types");
        // Cleanup must not fire on push:main or dispatch — that's the
        // deploy workflow's and the sweep's job.
        assert!(parsed["on"]["push"].is_null());
        assert!(parsed["on"]["workflow_dispatch"].is_null());
    }

    #[test]
    fn deletes_the_prefix_pr_number_worker() {
        let yaml = super::super::workflow::emit(&build(&fixture_spec()));
        // The deleted name must match what the deploy workflow's
        // `preview` job creates: `<previewScriptPrefix>-pr-<N>`.
        assert!(yaml.contains("--name example-notes-pr-${{ github.event.pull_request.number }}"));
        assert!(yaml.contains("cd apps/example-notes"));
        assert!(yaml.contains("|| true"), "delete is idempotent");
    }
}

//! Builder for `<name>-deploy.yml` — the Cloudflare Worker deploy
//! workflow.
//!
//! Five jobs in the generated file:
//!
//! 1. `changes` — paths filter; gates the other jobs to PRs/pushes
//!    that actually touch the project.
//! 2. `verify` — calls `cf-deploy.yml` with `verify_only: true`. A
//!    fast credential gate on every qualifying PR push.
//! 3. `preview` — calls `cf-deploy.yml` with a `script_name_override`
//!    derived from `previewScriptPrefix` and the PR number. Sequenced
//!    after `verify` so broken credentials don't burn a real deploy.
//! 4. `comment` — sticky PR comment with the preview Worker's
//!    `*.workers.dev` URL (sourced from `preview.outputs.worker_url`).
//! 5. `deploy` — calls `cf-deploy.yml` with `verify_only: false` on
//!    `push: main` or `workflow_dispatch`.
//!
//! The `cleanup` job that runs on `pull_request: closed` lives in a
//! hand-authored sibling file `<name>-cleanup.yml`: its trigger and
//! concern (Worker deletion) are orthogonal to the deploy lifecycle,
//! and the wrangler invocation is project-specific in ways the
//! schema would only awkwardly express.

use std::collections::BTreeMap;
use std::path::PathBuf;

use indexmap::IndexMap;
use serde::Deserialize;
use serde_yaml_ng::Value;

use super::workflow::{
    ActionStep, CancelInProgress, Concurrency, Empty, InlineJob, Job, Needs, On, PermissionLevel,
    Permissions, PushTrigger, ReusableCall, RunStep, Step, Workflow,
};

/// One Worker project's deploy workflow inputs.
pub struct WorkerDeploySpec {
    pub project_dir: PathBuf,
    pub project_name: String,
    pub deploy: CiDeploy,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CiDeploy {
    pub reusable_workflow: String,
    pub preview_script_prefix: String,
}

pub fn build(spec: &WorkerDeploySpec) -> Workflow {
    let mut jobs: IndexMap<String, Job> = IndexMap::new();
    jobs.insert("changes".to_string(), Job::Inline(changes_job(spec)));
    jobs.insert("verify".to_string(), Job::ReusableCall(verify_call(spec)));
    jobs.insert("preview".to_string(), Job::ReusableCall(preview_call(spec)));
    jobs.insert("comment".to_string(), Job::Inline(comment_job(spec)));
    jobs.insert("deploy".to_string(), Job::ReusableCall(deploy_call(spec)));

    Workflow {
        name: format!("{} deploy", spec.project_name),
        on: On {
            // Default PR types (opened/synchronize/reopened); the
            // `closed` event goes to the hand-authored cleanup
            // sibling so generated jobs don't carry redundant
            // `action != 'closed'` guards.
            pull_request: Some(Empty),
            push: Some(PushTrigger {
                branches: vec!["main".to_string()],
                paths: vec![],
            }),
            workflow_dispatch: Some(Empty),
        },
        // Caller permissions are the ceiling for the reusable
        // workflow's permissions. `id-token: write` is needed so the
        // called workflow's nix-installer can authenticate with
        // FlakeHub; `pull-requests: write` lets the `comment` job
        // post/edit the sticky URL comment.
        permissions: Some(Permissions {
            id_token: Some(PermissionLevel::Write),
            contents: Some(PermissionLevel::Read),
            pull_requests: Some(PermissionLevel::Write),
        }),
        // Serialize real deploys to this Worker; PR-triggered runs
        // use a per-PR group and cancel on update so a force-push
        // abandons the in-flight verify/preview.
        concurrency: Some(Concurrency {
            group: format!(
                "{}-deploy-${{{{ github.event_name == 'pull_request' && github.ref || 'main' }}}}",
                spec.project_name,
            ),
            cancel_in_progress: CancelInProgress::Expression(
                "${{ github.event_name == 'pull_request' }}".to_string(),
            ),
        }),
        jobs,
    }
}

// ── changes ───────────────────────────────────────────────────────────────

fn changes_job(spec: &WorkerDeploySpec) -> InlineJob {
    let project_dir_str = spec.project_dir.to_string_lossy().to_string();
    let mut outputs: BTreeMap<String, String> = BTreeMap::new();
    outputs.insert(
        "portfolio".to_string(),
        "${{ steps.filter.outputs.portfolio }}".to_string(),
    );

    let filter_body = format!(
        "portfolio:\n  - '{project_dir_str}/**'\n  - '.github/workflows/{name}-deploy.yml'\n  - '.github/workflows/{reusable}'\n",
        name = spec.project_name,
        reusable = filename_of(&spec.deploy.reusable_workflow),
    );

    let mut checkout_with: BTreeMap<String, Value> = BTreeMap::new();
    checkout_with.insert("fetch-depth".to_string(), Value::Number(0.into()));

    let mut filter_with: BTreeMap<String, Value> = BTreeMap::new();
    filter_with.insert(
        "base".to_string(),
        Value::String("${{ steps.base.outputs.ref }}".to_string()),
    );
    filter_with.insert("filters".to_string(), Value::String(filter_body));

    InlineJob {
        name: Some("Detect changes".to_string()),
        needs: Needs::Multiple(vec![]),
        if_: None,
        runs_on: "ubuntu-latest".to_string(),
        permissions: Some(Permissions {
            id_token: None,
            contents: Some(PermissionLevel::Read),
            pull_requests: Some(PermissionLevel::Read),
        }),
        env: BTreeMap::new(),
        outputs,
        steps: vec![
            Step::Action(ActionStep {
                uses: "actions/checkout@v6".to_string(),
                id: None,
                name: None,
                if_: None,
                with: checkout_with,
                env: BTreeMap::new(),
            }),
            // Base-ref shim: PR runs use pull_request.base.sha; push
            // runs use github.event.before. On a freshly created
            // branch `before` is the zero-SHA — fall back to HEAD^
            // then HEAD so the filter has a usable base.
            Step::Run(RunStep {
                id: Some("base".to_string()),
                name: None,
                if_: None,
                run: r#"BASE="${{ github.event.pull_request.base.sha || github.event.before }}"
if [[ -z "$BASE" || "$BASE" == "0000000000000000000000000000000000000000" ]]; then
  BASE=$(git rev-parse HEAD^ 2>/dev/null || git rev-parse HEAD)
fi
echo "ref=$BASE" >> "$GITHUB_OUTPUT"
"#
                .to_string(),
                env: BTreeMap::new(),
            }),
            Step::Action(ActionStep {
                uses: "dorny/paths-filter@v4".to_string(),
                id: Some("filter".to_string()),
                name: None,
                if_: None,
                with: filter_with,
                env: BTreeMap::new(),
            }),
        ],
    }
}

// ── verify / preview / deploy: cf-deploy.yml callers ─────────────────────

fn verify_call(spec: &WorkerDeploySpec) -> ReusableCall {
    let mut with: BTreeMap<String, Value> = BTreeMap::new();
    with.insert(
        "project_dir".to_string(),
        Value::String(spec.project_dir.to_string_lossy().to_string()),
    );
    with.insert("verify_only".to_string(), Value::Bool(true));
    ReusableCall {
        needs: Needs::Single("changes".to_string()),
        if_: Some(pr_gate()),
        uses: spec.deploy.reusable_workflow.clone(),
        with,
        secrets: op_secret(),
    }
}

fn preview_call(spec: &WorkerDeploySpec) -> ReusableCall {
    let mut with: BTreeMap<String, Value> = BTreeMap::new();
    with.insert(
        "project_dir".to_string(),
        Value::String(spec.project_dir.to_string_lossy().to_string()),
    );
    with.insert(
        "script_name_override".to_string(),
        Value::String(format!(
            "{}-pr-${{{{ github.event.pull_request.number }}}}",
            spec.deploy.preview_script_prefix,
        )),
    );
    ReusableCall {
        needs: Needs::Multiple(vec!["changes".to_string(), "verify".to_string()]),
        if_: Some(pr_gate()),
        uses: spec.deploy.reusable_workflow.clone(),
        with,
        secrets: op_secret(),
    }
}

fn deploy_call(spec: &WorkerDeploySpec) -> ReusableCall {
    let mut with: BTreeMap<String, Value> = BTreeMap::new();
    with.insert(
        "project_dir".to_string(),
        Value::String(spec.project_dir.to_string_lossy().to_string()),
    );
    with.insert("verify_only".to_string(), Value::Bool(false));
    // `workflow_dispatch` bypasses the changes filter so a manual
    // re-deploy isn't blocked by an unchanged push base. Real
    // `push: main` deploys still gate on the filter.
    ReusableCall {
        needs: Needs::Single("changes".to_string()),
        if_: Some(
            "(github.event_name == 'push' && needs.changes.outputs.portfolio == 'true') || github.event_name == 'workflow_dispatch'"
                .to_string(),
        ),
        uses: spec.deploy.reusable_workflow.clone(),
        with,
        secrets: op_secret(),
    }
}

fn pr_gate() -> String {
    "github.event_name == 'pull_request' && needs.changes.outputs.portfolio == 'true'".to_string()
}

fn op_secret() -> BTreeMap<String, String> {
    let mut s = BTreeMap::new();
    s.insert(
        "OP_SERVICE_ACCOUNT_TOKEN".to_string(),
        "${{ secrets.OP_SERVICE_ACCOUNT_TOKEN }}".to_string(),
    );
    s
}

// ── comment ───────────────────────────────────────────────────────────────

/// Sticky PR comment with the preview Worker URL. An HTML marker in
/// the body lets subsequent runs find and edit the existing comment
/// instead of posting a new one on every push. Both list and edit go
/// through the REST API so integer comment IDs round-trip cleanly
/// (`gh pr view --json comments` would return GraphQL node IDs that
/// the REST PATCH endpoint doesn't accept).
fn comment_job(spec: &WorkerDeploySpec) -> InlineJob {
    let marker = format!("<!-- preview-deploy:{} -->", spec.project_name);
    let body_template = format!(
        "{marker}\n📦 **Preview deploy** for this PR is live at:\n\n${{WORKER_URL}}\n\nUpdates on every push; deleted when the PR closes.",
    );
    let run_script = format!(
        r#"body="{body_template}"
existing=$(gh api "repos/$REPO/issues/$PR_NUMBER/comments" \
  --jq 'map(select(.body | contains("{marker}"))) | .[0].id // empty')
if [[ -n "$existing" ]]; then
  gh api "repos/$REPO/issues/comments/$existing" -X PATCH -f body="$body"
else
  gh api "repos/$REPO/issues/$PR_NUMBER/comments" -X POST -f body="$body"
fi
"#,
    );

    let mut env: BTreeMap<String, String> = BTreeMap::new();
    env.insert(
        "GH_TOKEN".to_string(),
        "${{ secrets.GITHUB_TOKEN }}".to_string(),
    );
    env.insert(
        "PR_NUMBER".to_string(),
        "${{ github.event.pull_request.number }}".to_string(),
    );
    env.insert("REPO".to_string(), "${{ github.repository }}".to_string());
    env.insert(
        "WORKER_URL".to_string(),
        "${{ needs.preview.outputs.worker_url }}".to_string(),
    );

    InlineJob {
        // Match the hand-authored shape — no display name on this job.
        name: None,
        needs: Needs::Multiple(vec!["changes".to_string(), "preview".to_string()]),
        if_: Some(
            "github.event_name == 'pull_request' && needs.changes.outputs.portfolio == 'true'"
                .to_string(),
        ),
        runs_on: "ubuntu-latest".to_string(),
        permissions: Some(Permissions {
            id_token: None,
            contents: None,
            pull_requests: Some(PermissionLevel::Write),
        }),
        env: BTreeMap::new(),
        outputs: BTreeMap::new(),
        steps: vec![Step::Run(RunStep {
            id: None,
            name: Some("post preview URL comment".to_string()),
            if_: None,
            run: run_script,
            env,
        })],
    }
}

fn filename_of(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .expect("reusable_workflow has a filename")
        .to_string()
}

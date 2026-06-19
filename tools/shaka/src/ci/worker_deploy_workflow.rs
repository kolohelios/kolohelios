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
//! generated sibling file `<name>-cleanup.yml` (see
//! `worker_cleanup_workflow.rs`): its trigger and concern (Worker
//! deletion) are orthogonal to the deploy lifecycle, so it gets its
//! own file, but both derive from the same `ci.deploy` block.

use std::collections::BTreeMap;
use std::path::PathBuf;

use indexmap::IndexMap;
use serde::Deserialize;
use serde_yaml_ng::Value;

use super::workflow::{
    ActionStep, CancelInProgress, Concurrency, Empty, InlineJob, Job, Needs, On, PermissionLevel,
    Permissions, PullRequestTrigger, PushTrigger, ReusableCall, RunStep, Step, Workflow,
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
    /// Optional wrangler environment for the production `deploy` job
    /// (`--env <environment>` in `cf-deploy.yml`). `verify`/`preview`
    /// never set it, so a prod-only `custom_domain` route under
    /// `[env.production]` stays off route-free previews.
    #[serde(default)]
    pub environment: Option<String>,
    /// The Cargo-workspace member crate (a subdir of `project_dir`) that
    /// `cf-deploy.yml` runs `worker-build` in, via its `worker_build_dir`
    /// input. Threaded into all three calls (verify/preview/deploy) so the
    /// Worker builds from the member crate rather than the virtual workspace
    /// root (which has no `[package]`). Unset → omitted, so worker-build runs in
    /// `project_dir` (the single-crate default).
    #[serde(default)]
    pub worker_build_dir: Option<String>,
    /// Worker runtime secret names pushed at deploy by
    /// `push-worker-secrets`. Modelled here only so this
    /// `deny_unknown_fields` struct (which `generate-workflows`
    /// deserializes `ci.deploy` through) tolerates the field; workflow
    /// generation never reads it (hence `allow(dead_code)`), so the
    /// emitted deploy workflow is unchanged.
    #[serde(default)]
    #[allow(dead_code)]
    pub secrets: Option<Vec<String>>,
}

pub fn build(spec: &WorkerDeploySpec) -> Workflow {
    let mut jobs: IndexMap<String, Job> = IndexMap::new();
    jobs.insert("changes".to_string(), Job::Inline(changes_job(spec)));
    jobs.insert("verify".to_string(), Job::ReusableCall(verify_call(spec)));
    jobs.insert("preview".to_string(), Job::ReusableCall(preview_call(spec)));
    jobs.insert("comment".to_string(), Job::Inline(comment_job(spec)));
    jobs.insert("deploy".to_string(), Job::ReusableCall(deploy_call(spec)));
    jobs.insert("gate".to_string(), Job::Inline(gate_job(spec)));

    Workflow {
        name: format!("{} deploy", spec.project_name),
        on: On {
            // Default PR types (opened/synchronize/reopened); the
            // `closed` event goes to the generated cleanup sibling
            // (`<name>-cleanup.yml`) so deploy jobs don't carry
            // redundant `action != 'closed'` guards.
            pull_request: Some(PullRequestTrigger::All(Empty)),
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

    // Watch the local reusable workflow so a refactor of `cf-deploy.yml`
    // re-runs this project's deploy. A cross-repo reference
    // (`<owner>/<repo>/.github/workflows/<name>.yml@<ref>`) points at a
    // file that doesn't exist in the consumer repo, so there's nothing
    // local to watch — drop the line; the app dir and this generated
    // workflow stay watched.
    let reusable_watch = if spec.deploy.reusable_workflow.starts_with("./") {
        format!(
            "\n  - '.github/workflows/{}'",
            filename_of(&spec.deploy.reusable_workflow),
        )
    } else {
        String::new()
    };
    let filter_body = format!(
        "portfolio:\n  - '{project_dir_str}/**'\n  - '.github/workflows/{name}-deploy.yml'{reusable_watch}\n",
        name = spec.project_name,
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
                uses: "actions/checkout@v7".to_string(),
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

/// Add `worker_build_dir` to a reusable-call `with:` block when the project set
/// it — the Cargo-workspace member crate cf-deploy runs `worker-build` in. Used
/// by all three calls (verify/preview/deploy) so the Worker always builds from
/// the member crate, not the virtual workspace root (which has no `[package]`).
fn insert_worker_build_dir(with: &mut BTreeMap<String, Value>, spec: &WorkerDeploySpec) {
    if let Some(dir) = &spec.deploy.worker_build_dir {
        with.insert("worker_build_dir".to_string(), Value::String(dir.clone()));
    }
}

fn verify_call(spec: &WorkerDeploySpec) -> ReusableCall {
    let mut with: BTreeMap<String, Value> = BTreeMap::new();
    with.insert(
        "project_dir".to_string(),
        Value::String(spec.project_dir.to_string_lossy().to_string()),
    );
    with.insert("verify_only".to_string(), Value::Bool(true));
    insert_worker_build_dir(&mut with, spec);
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
    insert_worker_build_dir(&mut with, spec);
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
    // Only the production deploy selects a wrangler environment;
    // `verify`/`preview` stay on the route-free top-level config so a
    // prod-only `custom_domain` route under `[env.<environment>]` never
    // lands on a `*.workers.dev` preview.
    if let Some(environment) = &spec.deploy.environment {
        with.insert(
            "environment".to_string(),
            Value::String(environment.clone()),
        );
    }
    insert_worker_build_dir(&mut with, spec);
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

/// PR-scoped gate for the credential-bearing jobs (`verify`/`preview`)
/// and the `comment` job that depends on them.
///
/// Excludes Dependabot: GitHub runs Dependabot-triggered workflows
/// against the isolated Dependabot secret store, where repo/org Actions
/// secrets — including `OP_SERVICE_ACCOUNT_TOKEN` — resolve to empty. The
/// `cf-deploy.yml` reusable workflow hard-fails on an empty token, so
/// these jobs can *never* succeed on a Dependabot PR; running them only
/// turns the required `gate` red on every dependency bump. Skipping them
/// (the `gate` job treats `skipped` as passing) lets a green `validate`
/// carry the bump. The real `deploy` job runs on `push: main` only, which
/// Dependabot never triggers, so it needs no such guard.
fn pr_gate() -> String {
    "github.event_name == 'pull_request' && needs.changes.outputs.portfolio == 'true' \
     && github.actor != 'dependabot[bot]'"
        .to_string()
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
        if_: Some(pr_gate()),
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

// ── gate ──────────────────────────────────────────────────────────────────

/// The always-run job a branch-protected consumer requires instead of
/// the path-conditional `verify`. `if: always()` lets it observe every
/// upstream outcome, and the `jq` filter passes when each is `success`
/// or `skipped` — so a root-only PR (where `changes` skips
/// `verify`/`preview`/`deploy`) still reaches a green required check
/// instead of deadlocking at `BLOCKED`. Mirrors `main.yaml`'s `Gate`,
/// but the context name is project-scoped so it never collides with
/// that bare `Gate`.
///
/// `comment` is intentionally excluded from `needs`: it's a cosmetic
/// preview-URL post, so a transient `gh api` failure there must not
/// block the merge. The merge-readiness signal is verify/preview/deploy.
///
/// Uses inline `jq` rather than `shaka ci gate` so the job stays
/// portable for cross-repo consumers that call `cf-deploy.yml` without
/// `shaka` on `PATH`.
fn gate_job(spec: &WorkerDeploySpec) -> InlineJob {
    InlineJob {
        name: Some(format!("{} deploy gate", spec.project_name)),
        needs: Needs::Multiple(vec![
            "changes".to_string(),
            "verify".to_string(),
            "preview".to_string(),
            "deploy".to_string(),
        ]),
        if_: Some("always()".to_string()),
        runs_on: "ubuntu-latest".to_string(),
        permissions: None,
        env: BTreeMap::new(),
        outputs: BTreeMap::new(),
        steps: vec![Step::Run(RunStep {
            id: None,
            name: None,
            if_: None,
            run: r#"echo '${{ toJson(needs) }}' | jq -e '
  to_entries
  | map(select(.value.result != "success" and .value.result != "skipped"))
  | if length == 0 then "Gate passed"
    else error("Gate failed: \(map(.key) | join(", "))")
    end
'"#
            .to_string(),
            env: BTreeMap::new(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_spec() -> WorkerDeploySpec {
        WorkerDeploySpec {
            project_dir: PathBuf::from("apps/portfolio"),
            project_name: "portfolio".to_string(),
            deploy: CiDeploy {
                reusable_workflow: "./.github/workflows/cf-deploy.yml".to_string(),
                preview_script_prefix: "portfolio".to_string(),
                environment: None,
                worker_build_dir: None,
                secrets: None,
            },
        }
    }

    fn emit_fixture() -> serde_yaml_ng::Value {
        let yaml = super::super::workflow::emit(&build(&fixture_spec()));
        serde_yaml_ng::from_str(&yaml).expect("valid YAML")
    }

    #[test]
    fn emits_lifecycle_jobs_and_gate() {
        let parsed = emit_fixture();
        let jobs = parsed["jobs"].as_mapping().expect("jobs is a mapping");
        // The deploy workflow is exactly these jobs; `cleanup` lives in
        // the generated sibling, not here.
        for job in ["changes", "verify", "preview", "comment", "deploy", "gate"] {
            assert!(jobs.contains_key(job), "missing job: {job}");
        }
        assert!(
            !jobs.contains_key("cleanup"),
            "cleanup belongs in the sibling file"
        );
        assert_eq!(parsed["name"].as_str(), Some("portfolio deploy"));
    }

    #[test]
    fn gate_runs_always_with_project_scoped_name() {
        let parsed = emit_fixture();
        let gate = &parsed["jobs"]["gate"];
        // `if: always()` so a path-filtered skip still resolves the
        // required check instead of leaving it absent.
        assert_eq!(gate["if"].as_str(), Some("always()"));
        // Project-scoped name so the context never collides with
        // `main.yaml`'s bare `Gate`.
        assert_eq!(gate["name"].as_str(), Some("portfolio deploy gate"));
    }

    #[test]
    fn gate_needs_verify_and_preview_but_not_comment() {
        let parsed = emit_fixture();
        let needs: Vec<&str> = parsed["jobs"]["gate"]["needs"]
            .as_sequence()
            .expect("gate needs list")
            .iter()
            .filter_map(|n| n.as_str())
            .collect();
        for job in ["changes", "verify", "preview", "deploy"] {
            assert!(needs.contains(&job), "gate should need {job}: {needs:?}");
        }
        // `comment` is cosmetic; a flaky URL post must not block merge.
        assert!(
            !needs.contains(&"comment"),
            "gate should not need comment: {needs:?}"
        );
    }

    #[test]
    fn gate_passes_on_skipped_upstreams() {
        // The jq filter treats `skipped` (and `success`) as passing — the
        // root-only-PR case where verify/preview/deploy never run.
        let parsed = emit_fixture();
        let run = parsed["jobs"]["gate"]["steps"][0]["run"]
            .as_str()
            .expect("gate has a run step");
        assert!(run.contains(r#".value.result != "success""#), "{run}");
        assert!(run.contains(r#".value.result != "skipped""#), "{run}");
    }

    #[test]
    fn triggers_on_pr_push_main_and_dispatch() {
        let parsed = emit_fixture();
        assert!(parsed["on"].get("pull_request").is_some());
        assert_eq!(parsed["on"]["push"]["branches"][0].as_str(), Some("main"));
        assert!(parsed["on"].get("workflow_dispatch").is_some());
    }

    #[test]
    fn permissions_allow_oidc_and_pr_comments() {
        let parsed = emit_fixture();
        let perms = &parsed["permissions"];
        assert_eq!(perms["id-token"].as_str(), Some("write"));
        assert_eq!(perms["contents"].as_str(), Some("read"));
        // The `comment` job needs write to post the sticky URL comment.
        assert_eq!(perms["pull-requests"].as_str(), Some("write"));
    }

    #[test]
    fn concurrency_group_is_per_project() {
        let parsed = emit_fixture();
        let group = parsed["concurrency"]["group"]
            .as_str()
            .expect("group is a string");
        assert!(group.starts_with("portfolio-deploy-"), "got: {group}");
    }

    #[test]
    fn verify_call_gates_on_pr_and_runs_verify_only() {
        let parsed = emit_fixture();
        let verify = &parsed["jobs"]["verify"];
        assert_eq!(
            verify["uses"].as_str(),
            Some("./.github/workflows/cf-deploy.yml")
        );
        assert_eq!(verify["with"]["verify_only"].as_bool(), Some(true));
        let if_ = verify["if"].as_str().expect("verify has an if");
        assert!(if_.contains("github.event_name == 'pull_request'"), "{if_}");
    }

    #[test]
    fn credential_jobs_skip_dependabot_prs() {
        // Dependabot runs can't read `OP_SERVICE_ACCOUNT_TOKEN` (isolated
        // secret store), so the secret-bearing jobs and the comment that
        // depends on them must skip — otherwise the required `gate` is red
        // on every dependency bump. `deploy` runs on push only, so it
        // carries no Dependabot guard.
        let parsed = emit_fixture();
        for job in ["verify", "preview", "comment"] {
            let if_ = parsed["jobs"][job]["if"]
                .as_str()
                .unwrap_or_else(|| panic!("{job} has an if"));
            assert!(
                if_.contains("github.actor != 'dependabot[bot]'"),
                "{job} should skip Dependabot PRs: {if_}"
            );
        }
        let deploy_if = parsed["jobs"]["deploy"]["if"]
            .as_str()
            .expect("deploy has an if");
        assert!(
            !deploy_if.contains("dependabot"),
            "deploy runs on push only, no Dependabot guard needed: {deploy_if}"
        );
    }

    #[test]
    fn preview_overrides_script_name_with_pr_number() {
        let parsed = emit_fixture();
        let preview = &parsed["jobs"]["preview"];
        assert_eq!(
            preview["with"]["script_name_override"].as_str(),
            Some("portfolio-pr-${{ github.event.pull_request.number }}")
        );
        // Sequenced after verify so broken creds don't burn a deploy.
        let needs = preview["needs"].as_sequence().expect("needs is a list");
        assert!(needs.iter().any(|n| n.as_str() == Some("verify")));
    }

    #[test]
    fn deploy_call_runs_on_push_main_or_dispatch_not_verify_only() {
        let parsed = emit_fixture();
        let deploy = &parsed["jobs"]["deploy"];
        assert_eq!(deploy["with"]["verify_only"].as_bool(), Some(false));
        let if_ = deploy["if"].as_str().expect("deploy has an if");
        assert!(if_.contains("github.event_name == 'push'"), "{if_}");
        assert!(
            if_.contains("github.event_name == 'workflow_dispatch'"),
            "{if_}"
        );
    }

    #[test]
    fn comment_job_carries_a_project_scoped_marker() {
        let yaml = super::super::workflow::emit(&build(&fixture_spec()));
        // The HTML marker lets later runs find and edit the existing
        // comment instead of posting a new one each push.
        assert!(yaml.contains("<!-- preview-deploy:portfolio -->"));
    }

    #[test]
    fn changes_filter_watches_project_and_reusable_workflow() {
        let parsed = emit_fixture();
        let filters = parsed["jobs"]["changes"]["steps"]
            .as_sequence()
            .expect("steps")
            .iter()
            .find_map(|s| s["with"].get("filters").and_then(|f| f.as_str()))
            .expect("paths-filter step with a filters body");
        assert!(filters.contains("apps/portfolio/**"), "{filters}");
        assert!(filters.contains("cf-deploy.yml"), "{filters}");
    }

    // ── cross-repo reusable workflow (external-repo consumers) ──────────

    /// An external consumer (e.g. buzzingo) that reuses this repo's
    /// `cf-deploy.yml` via a cross-repo `uses:` reference instead of a
    /// local `./...` path.
    fn cross_repo_spec() -> WorkerDeploySpec {
        WorkerDeploySpec {
            project_dir: PathBuf::from("apps/buzzingo"),
            project_name: "buzzingo".to_string(),
            deploy: CiDeploy {
                reusable_workflow: "kolohelios/kolohelios/.github/workflows/cf-deploy.yml@main"
                    .to_string(),
                preview_script_prefix: "buzzingo".to_string(),
                environment: None,
                worker_build_dir: Some("crates/buzzingo-server".to_string()),
                secrets: None,
            },
        }
    }

    fn emit_cross_repo() -> serde_yaml_ng::Value {
        let yaml = super::super::workflow::emit(&build(&cross_repo_spec()));
        serde_yaml_ng::from_str(&yaml).expect("valid YAML")
    }

    #[test]
    fn cross_repo_reference_passes_through_to_uses() {
        let parsed = emit_cross_repo();
        for job in ["verify", "preview", "deploy"] {
            assert_eq!(
                parsed["jobs"][job]["uses"].as_str(),
                Some("kolohelios/kolohelios/.github/workflows/cf-deploy.yml@main"),
                "job {job} should call the cross-repo reusable workflow"
            );
        }
    }

    #[test]
    fn cross_repo_filter_omits_the_reusable_workflow_path() {
        let parsed = emit_cross_repo();
        let filters = parsed["jobs"]["changes"]["steps"]
            .as_sequence()
            .expect("steps")
            .iter()
            .find_map(|s| s["with"].get("filters").and_then(|f| f.as_str()))
            .expect("paths-filter step with a filters body");
        // The app dir and the generated deploy workflow stay watched...
        assert!(filters.contains("apps/buzzingo/**"), "{filters}");
        assert!(
            filters.contains(".github/workflows/buzzingo-deploy.yml"),
            "{filters}"
        );
        // ...but the cross-repo reusable workflow isn't a local file, so
        // there's nothing to watch.
        assert!(!filters.contains("cf-deploy.yml"), "{filters}");
    }

    // ── wrangler environment (prod-only custom_domain routes) ───────────

    /// A consumer whose production `custom_domain` route lives under
    /// `[env.production]` selects that environment on the real deploy.
    fn env_spec() -> WorkerDeploySpec {
        WorkerDeploySpec {
            project_dir: PathBuf::from("apps/buzzingo"),
            project_name: "buzzingo".to_string(),
            deploy: CiDeploy {
                reusable_workflow: "./.github/workflows/cf-deploy.yml".to_string(),
                preview_script_prefix: "buzzingo".to_string(),
                environment: Some("production".to_string()),
                worker_build_dir: None,
                secrets: None,
            },
        }
    }

    #[test]
    fn deploy_job_sets_environment_when_configured() {
        let yaml = super::super::workflow::emit(&build(&env_spec()));
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).expect("valid YAML");
        assert_eq!(
            parsed["jobs"]["deploy"]["with"]["environment"].as_str(),
            Some("production")
        );
    }

    #[test]
    fn verify_and_preview_never_set_environment() {
        let yaml = super::super::workflow::emit(&build(&env_spec()));
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).expect("valid YAML");
        // The route-free top-level config serves PR previews/verifies; a
        // prod-only route under `[env.production]` must never reach them.
        assert!(parsed["jobs"]["verify"]["with"]["environment"].is_null());
        assert!(parsed["jobs"]["preview"]["with"]["environment"].is_null());
    }

    #[test]
    fn environment_omitted_when_unset() {
        // The portfolio fixture has `environment: None`; the deploy job's
        // `with` map carries no `environment` key (today's behavior).
        let parsed = emit_fixture();
        assert!(parsed["jobs"]["deploy"]["with"]["environment"].is_null());
    }

    #[test]
    fn worker_build_dir_emitted_in_all_three_calls() {
        // cross_repo_spec sets `worker_build_dir`; unlike `environment` (deploy
        // only), it must thread into every cf-deploy call so the Worker always
        // builds from the member crate, not the virtual workspace root.
        let parsed = emit_cross_repo();
        for job in ["verify", "preview", "deploy"] {
            assert_eq!(
                parsed["jobs"][job]["with"]["worker_build_dir"].as_str(),
                Some("crates/buzzingo-server"),
                "{job} should pass worker_build_dir"
            );
        }
    }

    #[test]
    fn worker_build_dir_omitted_when_unset() {
        // The portfolio fixture has `worker_build_dir: None`; no call carries it.
        let parsed = emit_fixture();
        for job in ["verify", "preview", "deploy"] {
            assert!(
                parsed["jobs"][job]["with"]["worker_build_dir"].is_null(),
                "{job} should omit worker_build_dir"
            );
        }
    }
}

//! Builder for the repo's single `.github/workflows/main.yaml`.
//!
//! Unlike `cloudflare-*-apply.yml` (one file per project), `main.yaml`
//! is a single workflow with one job per buildable project, an
//! aggregator gate job, and a shared preflight job. The shape of each
//! per-project build job comes from that project's `ci.build` block
//! (`#CiBuild` in `tools/shaka/schema/project-schema.cue`); the rest
//! of the file — the `changes` paths-filter job, the preflight job's
//! steps, the gate's body, top-level triggers + concurrency — is
//! fixed in this module.
//!
//! Rationale comments the hand-authored `main.yaml` carried live here
//! next to the code that emits each block, rather than in the
//! generated YAML.

use std::collections::BTreeMap;
use std::path::PathBuf;

use indexmap::IndexMap;
use serde::Deserialize;
use serde_yaml_ng::Value;

use super::workflow::{
    ActionStep, CancelInProgress, Concurrency, Empty, InlineJob, Job, Needs, On, PermissionLevel,
    Permissions, PushTrigger, RunStep, Step, Workflow,
};

/// One project's contribution to `main.yaml`: its filesystem location,
/// its declared name, and the parsed `ci.build` block.
pub struct MainBuildSpec {
    pub project_dir: PathBuf,
    pub project_name: String,
    pub build: CiBuild,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CiBuild {
    pub filter_key: String,
    pub job_id: String,
    pub display_name: String,
    #[serde(default = "default_nix_command")]
    pub nix_command: String,
    pub attr: Option<String>,
    pub publish: Publish,
    #[serde(default)]
    pub dispatch: Vec<Dispatch>,
}

fn default_nix_command() -> String {
    "build".to_string()
}

#[derive(Deserialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Publish {
    Flakehub(PublishFlakehub),
    Artifact(PublishArtifact),
}

#[derive(Deserialize, Debug, Clone)]
pub struct PublishFlakehub {
    pub name: String,
    pub visibility: String,
    pub rolling: bool,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PublishArtifact {
    pub name: String,
    pub path: String,
    pub retention_days: u32,
    pub compression_level: u32,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Dispatch {
    pub repo: String,
    pub event_type: String,
}

/// Emit the workflow tree for `main.yaml` from the project list.
/// Caller passes specs in the desired build-job order (currently
/// schema-discovery order, which is filesystem-sorted).
pub fn build(specs: &[MainBuildSpec]) -> Workflow {
    let mut jobs: IndexMap<String, Job> = IndexMap::new();
    jobs.insert("changes".to_string(), Job::Inline(changes_job(specs)));
    jobs.insert("preflight".to_string(), Job::Inline(preflight_job()));
    for spec in specs {
        let job_id = format!("build-{}", spec.build.job_id);
        jobs.insert(job_id, Job::Inline(build_job(spec)));
    }
    jobs.insert("gate".to_string(), Job::Inline(gate_job(specs)));

    Workflow {
        name: "CI".to_string(),
        on: On {
            pull_request: Some(Empty),
            push: Some(PushTrigger {
                branches: vec!["main".to_string()],
                paths: vec![],
            }),
            workflow_dispatch: Some(Empty),
        },
        permissions: None,
        concurrency: Some(Concurrency {
            group: "${{ github.workflow }}-${{ github.ref }}".to_string(),
            cancel_in_progress: CancelInProgress::Expression(
                "${{ github.event_name == 'pull_request' || github.ref != 'refs/heads/main' }}"
                    .to_string(),
            ),
        }),
        jobs,
    }
}

// ── changes ───────────────────────────────────────────────────────────────

/// Detect changed paths so build jobs can skip when their inputs are
/// untouched. Preflight always runs but passes the base ref to
/// `shaka preflight --since` so individual checks self-skip.
fn changes_job(specs: &[MainBuildSpec]) -> InlineJob {
    let mut outputs: BTreeMap<String, String> = BTreeMap::new();
    outputs.insert(
        "base_ref".to_string(),
        "${{ steps.base.outputs.ref }}".to_string(),
    );
    for spec in specs {
        outputs.insert(
            spec.build.filter_key.clone(),
            format!("${{{{ steps.filter.outputs.{} }}}}", spec.build.filter_key),
        );
    }

    // Build the filter-spec YAML body as a multiline string so
    // `dorny/paths-filter` reads it.
    let mut filter_body = String::new();
    for spec in specs {
        filter_body.push_str(&format!("{}:\n", spec.build.filter_key));
        filter_body.push_str(&format!(
            "  - '{}/**'\n",
            spec.project_dir.to_string_lossy()
        ));
    }

    let mut checkout_with: BTreeMap<String, Value> = BTreeMap::new();
    checkout_with.insert("fetch-depth".to_string(), Value::Number(0.into()));

    let mut filter_with: BTreeMap<String, Value> = BTreeMap::new();
    filter_with.insert(
        "base".to_string(),
        Value::String("${{ steps.base.outputs.ref }}".to_string()),
    );
    filter_with.insert("filters".to_string(), Value::String(filter_body));

    InlineJob {
        name: "Detect changes".to_string(),
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
            // The base ref for PR runs is `pull_request.base.sha`; for
            // push runs `github.event.before`. On the first push to a
            // branch `before` is 40 zeros — fall back to `HEAD^` then
            // `HEAD` so the filter still has a usable base.
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

// ── preflight ─────────────────────────────────────────────────────────────

/// Heaviest upstream of `gate`: runs `shaka commit lint` (PR only)
/// and `shaka preflight --since <base>`. Env vars include the
/// devbox-specific `TF_VAR_*` set used by `tofu validate` inside
/// `just validate`; if a future project needs different env, encode
/// it in the schema.
fn preflight_job() -> InlineJob {
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    env.insert(
        "TF_VAR_linode_token".to_string(),
        "${{ secrets.LINODE_TOKEN }}".to_string(),
    );
    env.insert(
        "TF_VAR_root_pass".to_string(),
        "${{ secrets.DEVBOX_ROOT_PASS }}".to_string(),
    );
    env.insert(
        "TF_VAR_image_id".to_string(),
        "${{ vars.DEVBOX_IMAGE_ID }}".to_string(),
    );

    let mut checkout_with: BTreeMap<String, Value> = BTreeMap::new();
    checkout_with.insert("fetch-depth".to_string(), Value::Number(0.into()));

    let mut nix_install_with: BTreeMap<String, Value> = BTreeMap::new();
    nix_install_with.insert("determinate".to_string(), Value::Bool(true));
    nix_install_with.insert("flakehub".to_string(), Value::Bool(true));

    // Skip cargo dependency rebuilds when Cargo.lock is unchanged.
    // Only covers host-side cargo from `just validate`; the
    // `nix flake check` path runs in a sandbox actions/cache can't
    // reach.
    let mut cargo_cache_with: BTreeMap<String, Value> = BTreeMap::new();
    cargo_cache_with.insert(
        "path".to_string(),
        Value::String("tools/shaka/target/\n~/.cargo/registry/\n~/.cargo/git/\n".to_string()),
    );
    cargo_cache_with.insert(
        "key".to_string(),
        Value::String(
            "cargo-${{ runner.os }}-${{ hashFiles('tools/shaka/Cargo.lock') }}".to_string(),
        ),
    );
    cargo_cache_with.insert(
        "restore-keys".to_string(),
        Value::String("cargo-${{ runner.os }}-".to_string()),
    );

    InlineJob {
        name: "Preflight".to_string(),
        needs: Needs::Single("changes".to_string()),
        if_: None,
        runs_on: "ubuntu-latest".to_string(),
        permissions: Some(Permissions {
            id_token: Some(PermissionLevel::Write),
            contents: Some(PermissionLevel::Read),
            pull_requests: None,
        }),
        env,
        outputs: BTreeMap::new(),
        steps: vec![
            Step::Action(ActionStep {
                uses: "actions/checkout@v6".to_string(),
                id: None,
                name: None,
                if_: None,
                with: checkout_with,
                env: BTreeMap::new(),
            }),
            Step::Action(ActionStep {
                uses: "DeterminateSystems/nix-installer-action@main".to_string(),
                id: None,
                name: None,
                if_: None,
                with: nix_install_with,
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
            Step::Action(ActionStep {
                uses: "actions/cache@v5".to_string(),
                id: None,
                name: None,
                if_: None,
                with: cargo_cache_with,
                env: BTreeMap::new(),
            }),
            // `actions/checkout` produces a git-only tree; shaka's
            // preflight and commit lint both shell out to jj, so
            // colocate before either runs.
            Step::Run(RunStep {
                id: None,
                name: Some("Colocate jj on the git checkout".to_string()),
                if_: None,
                run: "nix develop ./tools/shaka --command jj git init --colocate".to_string(),
                env: BTreeMap::new(),
            }),
            Step::Run(RunStep {
                id: None,
                name: Some("shaka commit lint".to_string()),
                if_: Some("github.event_name == 'pull_request'".to_string()),
                run: r#"nix develop ./tools/shaka --command tools/shaka/bin/shaka commit lint \
  -r '${{ github.event.pull_request.base.sha }}..${{ github.event.pull_request.head.sha }}'
"#
                .to_string(),
                env: BTreeMap::new(),
            }),
            Step::Run(RunStep {
                id: None,
                name: Some("shaka preflight".to_string()),
                if_: None,
                run: r#"nix develop ./tools/shaka --command tools/shaka/bin/shaka preflight \
  --since "${{ needs.changes.outputs.base_ref }}"
"#
                .to_string(),
                env: BTreeMap::new(),
            }),
        ],
    }
}

// ── build-<id> ────────────────────────────────────────────────────────────

/// Per-project build job: checkout, nix install, build, publish.
/// `if:` lets the job skip on PR runs whose paths don't touch this
/// project (path filter from the `changes` job).
fn build_job(spec: &MainBuildSpec) -> InlineJob {
    let project_dir_str = spec.project_dir.to_string_lossy().to_string();
    let if_cond = format!(
        "github.event_name != 'pull_request' || needs.changes.outputs.{} == 'true'",
        spec.build.filter_key,
    );
    let publish_if = "github.event_name == 'push' || github.event_name == 'workflow_dispatch'";

    let mut nix_install_with: BTreeMap<String, Value> = BTreeMap::new();
    nix_install_with.insert("determinate".to_string(), Value::Bool(true));
    nix_install_with.insert("flakehub".to_string(), Value::Bool(true));

    let nix_cmd_verb = if spec.build.nix_command == "check" {
        "flake check"
    } else {
        "build"
    };
    let attr_suffix = spec
        .build
        .attr
        .as_ref()
        .map(|a| format!("#{a}"))
        .unwrap_or_default();
    let nix_run = format!("nix {nix_cmd_verb} ./{project_dir_str}{attr_suffix}");

    let mut steps: Vec<Step> = vec![
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
            with: nix_install_with,
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
        Step::Run(RunStep {
            id: None,
            name: None,
            if_: None,
            run: nix_run,
            env: BTreeMap::new(),
        }),
    ];

    match &spec.build.publish {
        Publish::Flakehub(fh) => {
            let mut with: BTreeMap<String, Value> = BTreeMap::new();
            with.insert(
                "directory".to_string(),
                Value::String(project_dir_str.clone()),
            );
            with.insert("name".to_string(), Value::String(fh.name.clone()));
            with.insert(
                "visibility".to_string(),
                Value::String(fh.visibility.clone()),
            );
            with.insert("rolling".to_string(), Value::Bool(fh.rolling));
            steps.push(Step::Action(ActionStep {
                uses: "DeterminateSystems/flakehub-push@main".to_string(),
                id: None,
                name: None,
                if_: Some(publish_if.to_string()),
                with,
                env: BTreeMap::new(),
            }));
        }
        Publish::Artifact(art) => {
            let mut with: BTreeMap<String, Value> = BTreeMap::new();
            with.insert("name".to_string(), Value::String(art.name.clone()));
            with.insert("path".to_string(), Value::String(art.path.clone()));
            with.insert(
                "retention-days".to_string(),
                Value::Number(art.retention_days.into()),
            );
            with.insert(
                "compression-level".to_string(),
                Value::Number(art.compression_level.into()),
            );
            steps.push(Step::Action(ActionStep {
                uses: "actions/upload-artifact@v7".to_string(),
                id: None,
                name: None,
                if_: Some(publish_if.to_string()),
                with,
                env: BTreeMap::new(),
            }));
        }
    }

    for dispatch in &spec.build.dispatch {
        steps.extend(dispatch_steps(&spec.project_name, dispatch, publish_if));
    }

    InlineJob {
        name: format!("Build {}", spec.build.display_name),
        needs: Needs::Multiple(vec!["preflight".to_string(), "changes".to_string()]),
        if_: Some(if_cond),
        runs_on: "ubuntu-latest".to_string(),
        permissions: Some(Permissions {
            id_token: Some(PermissionLevel::Write),
            contents: Some(PermissionLevel::Read),
            pull_requests: None,
        }),
        env: BTreeMap::new(),
        outputs: BTreeMap::new(),
        steps,
    }
}

/// Mint a `kolohelios-bot` GitHub App token and POST to the target
/// repo's `/dispatches` endpoint. Authentication is fixed (App is the
/// same identity used by auto-rebase-prs and the bump-locks workflows
/// across repos) so the `ci.build.dispatch` block only declares the
/// target.
fn dispatch_steps(project_name: &str, dispatch: &Dispatch, publish_if: &str) -> Vec<Step> {
    // `kolohelios/blogs-and-posts` → owner `kolohelios`, repo
    // `blogs-and-posts`. The App-token action takes the short repo
    // name in `repositories:`; the `gh api` URL takes the full path.
    let (owner, short_repo) = match dispatch.repo.split_once('/') {
        Some((o, r)) => (o.to_string(), r.to_string()),
        None => (String::new(), dispatch.repo.clone()),
    };

    let mut token_with: BTreeMap<String, Value> = BTreeMap::new();
    token_with.insert(
        "client-id".to_string(),
        Value::String("${{ vars.KOLOHELIOS_BOT_CLIENT_ID }}".to_string()),
    );
    token_with.insert(
        "private-key".to_string(),
        Value::String("${{ secrets.KOLOHELIOS_BOT_APP_PRIVATE_KEY }}".to_string()),
    );
    token_with.insert("owner".to_string(), Value::String(owner));
    token_with.insert(
        "repositories".to_string(),
        Value::String(short_repo.clone()),
    );

    let mut env: BTreeMap<String, String> = BTreeMap::new();
    env.insert(
        "GH_TOKEN".to_string(),
        "${{ steps.bot-token.outputs.token }}".to_string(),
    );

    vec![
        Step::Action(ActionStep {
            uses: "actions/create-github-app-token@v3".to_string(),
            id: Some("bot-token".to_string()),
            name: None,
            if_: Some(publish_if.to_string()),
            with: token_with,
            env: BTreeMap::new(),
        }),
        Step::Run(RunStep {
            id: None,
            name: Some(format!("Notify {short_repo} of new {project_name} publish")),
            if_: Some(publish_if.to_string()),
            run: format!(
                "gh api -X POST repos/{}/dispatches \\\n  -f event_type={}\n",
                dispatch.repo, dispatch.event_type,
            ),
            env,
        }),
    ]
}

// ── gate ──────────────────────────────────────────────────────────────────

/// The single job branch protection requires. `if: always()` lets it
/// see the actual status of every upstream (success, skipped, failed)
/// so path-filtered skips still let the merge gate go green.
/// `shaka ci gate` examines `${{ toJson(needs) }}` and exits non-zero
/// on any blocking outcome.
fn gate_job(specs: &[MainBuildSpec]) -> InlineJob {
    let mut needs_list = vec!["changes".to_string(), "preflight".to_string()];
    for spec in specs {
        needs_list.push(format!("build-{}", spec.build.job_id));
    }

    let mut nix_install_with: BTreeMap<String, Value> = BTreeMap::new();
    nix_install_with.insert("determinate".to_string(), Value::Bool(true));
    nix_install_with.insert("flakehub".to_string(), Value::Bool(true));

    InlineJob {
        name: "Gate".to_string(),
        needs: Needs::Multiple(needs_list),
        if_: Some("always()".to_string()),
        runs_on: "ubuntu-latest".to_string(),
        permissions: Some(Permissions {
            id_token: Some(PermissionLevel::Write),
            contents: Some(PermissionLevel::Read),
            pull_requests: None,
        }),
        env: BTreeMap::new(),
        outputs: BTreeMap::new(),
        steps: vec![
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
                with: nix_install_with,
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
            Step::Run(RunStep {
                id: None,
                name: None,
                if_: None,
                run: r#"nix run ./tools/shaka -- ci gate --needs '${{ toJson(needs) }}'"#
                    .to_string(),
                env: BTreeMap::new(),
            }),
        ],
    }
}

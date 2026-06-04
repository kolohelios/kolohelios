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
    Permissions, PullRequestTrigger, PushTrigger, RunStep, Step, Workflow,
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
#[serde(rename_all = "camelCase")]
pub struct PublishFlakehub {
    pub name: String,
    pub visibility: String,
    pub rolling: bool,
    #[serde(default)]
    pub populate_consumer_cache: bool,
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
            pull_request: Some(PullRequestTrigger::All(Empty)),
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
        name: Some("Preflight".to_string()),
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
        "github.event_name == 'workflow_dispatch' || needs.changes.outputs.{} == 'true'",
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
            // After flakehub-push registers the new revision, rebuild
            // from the FlakeHub URL so the closure pushed to FlakeHub
            // Cache matches what downstream consumers will ask for.
            // The local-path `nix build` above produces a different
            // `.drv` (different source-store-hash for path vs tarball
            // inputs), so its cached output never satisfies consumer
            // lookups. See kolohelios/kolohelios#591.
            if fh.populate_consumer_cache {
                steps.push(Step::Run(RunStep {
                    id: None,
                    name: Some(format!(
                        "Build {} from FlakeHub URL (populate consumer cache)",
                        spec.build.display_name
                    )),
                    if_: Some(publish_if.to_string()),
                    run: format!("nix build 'https://flakehub.com/f/{}/*.tar.gz'", fh.name),
                    env: BTreeMap::new(),
                }));
            }
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
        name: Some(format!("Build {}", spec.build.display_name)),
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

    InlineJob {
        name: Some("Gate".to_string()),
        needs: Needs::Multiple(needs_list),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn flakehub_spec() -> MainBuildSpec {
        MainBuildSpec {
            project_dir: PathBuf::from("tools/shaka"),
            project_name: "shaka".to_string(),
            build: CiBuild {
                filter_key: "shaka".to_string(),
                job_id: "shaka".to_string(),
                display_name: "shaka".to_string(),
                nix_command: "build".to_string(),
                attr: None,
                publish: Publish::Flakehub(PublishFlakehub {
                    name: "kolohelios/shaka".to_string(),
                    visibility: "public".to_string(),
                    rolling: true,
                    populate_consumer_cache: false,
                }),
                dispatch: vec![],
            },
        }
    }

    fn parse(specs: &[MainBuildSpec]) -> serde_yaml_ng::Value {
        let yaml = super::super::workflow::emit(&build(specs));
        serde_yaml_ng::from_str(&yaml).expect("valid YAML")
    }

    #[test]
    fn always_emits_changes_preflight_and_gate() {
        let parsed = parse(&[flakehub_spec()]);
        let jobs = parsed["jobs"].as_mapping().expect("jobs mapping");
        for job in ["changes", "preflight", "gate"] {
            assert!(jobs.contains_key(job), "missing fixed job: {job}");
        }
        assert_eq!(parsed["name"].as_str(), Some("CI"));
    }

    #[test]
    fn build_job_id_is_prefixed_and_gate_depends_on_it() {
        let parsed = parse(&[flakehub_spec()]);
        assert!(parsed["jobs"].get("build-shaka").is_some());
        let needs = parsed["jobs"]["gate"]["needs"]
            .as_sequence()
            .expect("gate needs list");
        assert!(needs.iter().any(|n| n.as_str() == Some("build-shaka")));
        // `gate` runs even when upstreams skip, so branch protection
        // stays green on path-filtered PRs.
        assert_eq!(parsed["jobs"]["gate"]["if"].as_str(), Some("always()"));
    }

    #[test]
    fn flakehub_publish_step_carries_name_and_visibility() {
        let parsed = parse(&[flakehub_spec()]);
        let steps = parsed["jobs"]["build-shaka"]["steps"]
            .as_sequence()
            .expect("steps");
        let push = steps
            .iter()
            .find(|s| s["uses"].as_str() == Some("DeterminateSystems/flakehub-push@main"))
            .expect("flakehub-push step");
        assert_eq!(push["with"]["name"].as_str(), Some("kolohelios/shaka"));
        assert_eq!(push["with"]["visibility"].as_str(), Some("public"));
        assert_eq!(push["with"]["rolling"].as_bool(), Some(true));
    }

    #[test]
    fn populate_consumer_cache_adds_a_flakehub_url_rebuild() {
        let mut spec = flakehub_spec();
        if let Publish::Flakehub(ref mut fh) = spec.build.publish {
            fh.populate_consumer_cache = true;
        }
        let yaml = super::super::workflow::emit(&build(&[spec]));
        assert!(yaml.contains("nix build 'https://flakehub.com/f/kolohelios/shaka/*.tar.gz'"));
    }

    #[test]
    fn artifact_publish_uploads_with_retention() {
        let mut spec = flakehub_spec();
        spec.build.publish = Publish::Artifact(PublishArtifact {
            name: "site".to_string(),
            path: "dist".to_string(),
            retention_days: 7,
            compression_level: 6,
        });
        let yaml = super::super::workflow::emit(&build(&[spec]));
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).unwrap();
        let steps = parsed["jobs"]["build-shaka"]["steps"]
            .as_sequence()
            .unwrap();
        let upload = steps
            .iter()
            .find(|s| s["uses"].as_str() == Some("actions/upload-artifact@v7"))
            .expect("upload-artifact step");
        assert_eq!(upload["with"]["name"].as_str(), Some("site"));
        assert_eq!(upload["with"]["retention-days"].as_u64(), Some(7));
    }

    #[test]
    fn nix_check_command_uses_flake_check_verb() {
        let mut spec = flakehub_spec();
        spec.build.nix_command = "check".to_string();
        let yaml = super::super::workflow::emit(&build(&[spec]));
        assert!(yaml.contains("nix flake check ./tools/shaka"));
    }

    #[test]
    fn attr_suffix_appends_to_the_nix_target() {
        let mut spec = flakehub_spec();
        spec.build.attr = Some("packages.x86_64-linux.default".to_string());
        let yaml = super::super::workflow::emit(&build(&[spec]));
        assert!(yaml.contains("nix build ./tools/shaka#packages.x86_64-linux.default"));
    }

    #[test]
    fn dispatch_mints_a_bot_token_and_posts_to_target_repo() {
        let mut spec = flakehub_spec();
        spec.build.dispatch = vec![Dispatch {
            repo: "kolohelios/blogs-and-posts".to_string(),
            event_type: "shaka-published".to_string(),
        }];
        let yaml = super::super::workflow::emit(&build(&[spec]));
        assert!(yaml.contains("actions/create-github-app-token@v3"));
        assert!(yaml.contains("repos/kolohelios/blogs-and-posts/dispatches"));
        assert!(yaml.contains("event_type=shaka-published"));
    }

    #[test]
    fn changes_filter_lists_each_project_filter_key() {
        let parsed = parse(&[flakehub_spec()]);
        let filters = parsed["jobs"]["changes"]["steps"]
            .as_sequence()
            .unwrap()
            .iter()
            .find_map(|s| s["with"].get("filters").and_then(|f| f.as_str()))
            .expect("filters body");
        assert!(filters.contains("shaka:"), "{filters}");
        assert!(filters.contains("tools/shaka/**"), "{filters}");
    }
}

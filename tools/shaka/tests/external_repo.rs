//! Integration test for a domain-free external repo (#863).
//!
//! An external consumer (buzzingo) that ships no custom domains has no
//! `infra/cloudflare-dns/domains` registry — yet the project schema
//! hard-imports that package. We stage a temp repo with its own
//! `cue.mod` (module `kolohelios.com`) and a single domain-free
//! `rust-worker` project, but *no* registry, then drive the binary
//! against it: `schema_check` must assemble a permissive stub instead of
//! failing on a `cannot find package` import, and `ci generate-workflows`
//! must round-trip with no drift. Both route through
//! `schema_check::cue_project`, the shared chokepoint every `project.cue`
//! consumer (`schema-check`, `generate-justfiles`, `generate-workflows`,
//! `audit`) funnels through, so a green pass here covers them all.

use std::path::{Path, PathBuf};
use std::process::Command;

const SHAKA_BIN: &str = env!("CARGO_BIN_EXE_shaka");

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/external-repo")
}

/// Stage a domain-free external repo: a `cue.mod` declaring module
/// `kolohelios.com` (so the schema's absolute import has a module to
/// resolve against), a root LICENSE, and one `rust-worker` project — but
/// deliberately *no* `infra/cloudflare-dns` registry. This is the shape
/// the registry stub has to make validate.
fn staged_repo() -> tempfile::TempDir {
    let root = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(root.path().join("cue.mod")).unwrap();
    std::fs::write(
        root.path().join("cue.mod/module.cue"),
        "module: \"kolohelios.com\"\nlanguage: version: \"v0.15.1\"\n",
    )
    .unwrap();
    std::fs::write(root.path().join("LICENSE"), "MIT OR Apache-2.0\n").unwrap();

    let dst = root.path().join("apps/buzzingo");
    std::fs::create_dir_all(&dst).unwrap();
    std::fs::copy(
        fixtures_root().join("buzzingo/project.cue.fixture"),
        dst.join("project.cue"),
    )
    .unwrap();

    root
}

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(SHAKA_BIN)
        .args(args)
        .current_dir(root)
        .output()
        .expect("spawn shaka")
}

#[test]
fn schema_check_passes_without_a_domain_registry() {
    let repo = staged_repo();
    let out = run(repo.path(), &["project", "schema-check"]);
    assert!(
        out.status.success(),
        "schema-check failed for a domain-free repo: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The buzzingo project validated against the stub registry.
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("apps/buzzingo"), "{stdout}");
}

#[test]
fn generate_workflows_round_trips_without_a_domain_registry() {
    let repo = staged_repo();
    // First emit the deploy/cleanup workflows from the ci.deploy block...
    let gen = run(repo.path(), &["ci", "generate-workflows"]);
    assert!(
        gen.status.success(),
        "generate-workflows failed for a domain-free repo: {}",
        String::from_utf8_lossy(&gen.stderr)
    );
    // ...the cross-repo deploy workflow is emitted (proves #868 + #863
    // compose: a domain-free external repo generates its deploy wrapper).
    let deploy = repo.path().join(".github/workflows/buzzingo-deploy.yml");
    let body = std::fs::read_to_string(&deploy).expect("buzzingo-deploy.yml written");
    assert!(
        body.contains("kolohelios/kolohelios/.github/workflows/cf-deploy.yml@main"),
        "{body}"
    );
    // ...and `--check` agrees byte-for-byte right after generating.
    let check = run(repo.path(), &["ci", "generate-workflows", "--check"]);
    assert!(
        check.status.success(),
        "--check drifted right after generate: {}",
        String::from_utf8_lossy(&check.stderr)
    );
}

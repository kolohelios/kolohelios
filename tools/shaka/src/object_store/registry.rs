//! Aggregate the namespace registry by walking every `project.cue` and
//! reading its `objectStorage.namespaces` declaration.
//!
//! Decentralized declaration (each project owns its own demand) plus
//! centralized aggregation (shaka validates uniqueness and audits drift)
//! keeps the demand adjacent to the project that needs it without losing
//! global protection.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use snafu::{ResultExt, Snafu};

use crate::project::schema_check;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub struct Namespace {
    pub kind: String,
    pub name: String,
    pub purpose: String,
}

impl Namespace {
    /// The bucket prefix for this namespace, e.g. `tfstate/devbox`.
    pub fn prefix(&self) -> String {
        format!("{}/{}", self.kind, self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub project: PathBuf,
    pub namespace: Namespace,
}

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum RegistryError {
    #[snafu(display("could not write schema: {source}"))]
    WriteSchema { source: std::io::Error },

    #[snafu(display("failed to run cue export {file}: {source}"))]
    SpawnCueExport {
        file: String,
        source: std::io::Error,
    },

    #[snafu(display("cue export {file}: {stderr}"))]
    CueExport { file: String, stderr: String },

    #[snafu(display("failed to parse {file} as JSON: {source}"))]
    ParseProject {
        file: String,
        source: serde_json::Error,
    },
}

#[derive(Debug, Deserialize, Default)]
struct ProjectFile {
    #[serde(default, rename = "objectStorage")]
    object_storage: Option<ObjectStorageBlock>,
}

#[derive(Debug, Deserialize, Default)]
struct ObjectStorageBlock {
    #[serde(default)]
    namespaces: Vec<Namespace>,
}

/// Walk every project.cue under `root` and collect declared namespaces.
/// Returns entries sorted by `(kind, name, project)` for stable output.
pub fn collect(root: &Path) -> Result<Vec<Entry>, RegistryError> {
    let schema_path = schema_check::write_schema().context(WriteSchemaSnafu)?;
    let mut entries = Vec::new();
    for project_dir in schema_check::discover(root) {
        let project_file = project_dir.join("project.cue");
        if !project_file.exists() {
            continue;
        }
        let parsed = read_project(&schema_path, &project_file)?;
        let Some(block) = parsed.object_storage else {
            continue;
        };
        for ns in block.namespaces {
            entries.push(Entry {
                project: project_dir.clone(),
                namespace: ns,
            });
        }
    }
    entries.sort_by(|a, b| {
        a.namespace
            .kind
            .cmp(&b.namespace.kind)
            .then_with(|| a.namespace.name.cmp(&b.namespace.name))
            .then_with(|| a.project.cmp(&b.project))
    });
    Ok(entries)
}

fn read_project(schema: &Path, file: &Path) -> Result<ProjectFile, RegistryError> {
    // `cue export` needs both the schema (for #Project) and the project
    // file unified together. Schema-check has already enforced structure,
    // so any export failure here is a real problem and bubbles up.
    //
    // `current_dir(file.parent())` anchors cue's module-resolution walk
    // at the project dir. The project schema imports the domain
    // registry, so cue needs to find `cue.mod/module.cue` somewhere
    // up the tree. Inheriting the caller's cwd works in
    // `cargo test`/`cargo run` (cwd = repo root) but breaks under
    // `nix build`'s sandbox (cwd = `/build/.tmpXXX/`, no cue.mod). The
    // file's parent always sits inside the project tree containing
    // `cue.mod`, so the walk succeeds in both worlds.
    let project_dir = file.parent().expect("project file has a parent dir");
    let output = Command::new("cue")
        .args(["export", "--out", "json"])
        .arg(schema)
        .arg(file)
        .current_dir(project_dir)
        .output()
        .with_context(|_| SpawnCueExportSnafu {
            file: file.display().to_string(),
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return CueExportSnafu {
            file: file.display().to_string(),
            stderr: stderr.trim().to_string(),
        }
        .fail();
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(ProjectFile::default());
    }
    serde_json::from_str(&stdout).with_context(|_| ParseProjectSnafu {
        file: file.display().to_string(),
    })
}

/// Cross-project validation: same `(kind, name)` declared by two projects
/// is a hard error, and within a kind one name being a kebab-prefix of
/// another (e.g. `core` and `core-extra`) is an error so listings stay
/// unambiguous.
pub fn validate_uniqueness(entries: &[Entry]) -> Vec<String> {
    let mut errors = Vec::new();
    for (i, a) in entries.iter().enumerate() {
        for b in &entries[i + 1..] {
            if a.namespace.kind == b.namespace.kind && a.namespace.name == b.namespace.name {
                errors.push(format!(
                    "namespace '{}' declared by both {} and {}",
                    a.namespace.prefix(),
                    a.project.display(),
                    b.project.display()
                ));
            }
        }
    }
    for (i, a) in entries.iter().enumerate() {
        for b in &entries[i + 1..] {
            if a.namespace.kind != b.namespace.kind || a.namespace.name == b.namespace.name {
                continue;
            }
            let (short, long) = if a.namespace.name.len() <= b.namespace.name.len() {
                (&a.namespace, &b.namespace)
            } else {
                (&b.namespace, &a.namespace)
            };
            if long.name.starts_with(&format!("{}-", short.name)) {
                errors.push(format!(
                    "namespace name collision in kind '{}': '{}' is a kebab-prefix of '{}'",
                    a.namespace.kind, short.name, long.name
                ));
            }
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn parse_project_error_exposes_serde_source() {
        // Demonstrates that snafu's #[snafu(source)] field threads the
        // underlying serde_json::Error through std::error::Error::source(),
        // so callers can downcast / walk the chain instead of grepping
        // a flattened format! string.
        let err: RegistryError = serde_json::from_str::<ProjectFile>("not json")
            .with_context(|_| ParseProjectSnafu {
                file: "fixture.cue".to_string(),
            })
            .unwrap_err();
        let source = err.source().expect("source should be present");
        assert!(
            source.downcast_ref::<serde_json::Error>().is_some(),
            "source should downcast to serde_json::Error, got {source:?}"
        );
    }

    fn ns(kind: &str, name: &str) -> Namespace {
        Namespace {
            kind: kind.into(),
            name: name.into(),
            purpose: "test".into(),
        }
    }

    fn entry(project: &str, kind: &str, name: &str) -> Entry {
        Entry {
            project: PathBuf::from(project),
            namespace: ns(kind, name),
        }
    }

    #[test]
    fn validate_uniqueness_accepts_distinct() {
        let entries = vec![
            entry("infra/devbox", "tfstate", "devbox"),
            entry("infra/cf", "tfstate", "cloudflare"),
        ];
        assert!(validate_uniqueness(&entries).is_empty());
    }

    #[test]
    fn validate_uniqueness_rejects_duplicate_kind_name() {
        let entries = vec![
            entry("infra/devbox", "tfstate", "devbox"),
            entry("infra/other", "tfstate", "devbox"),
        ];
        let errs = validate_uniqueness(&entries);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("tfstate/devbox"));
    }

    #[test]
    fn validate_uniqueness_rejects_kebab_prefix_collision_same_kind() {
        let entries = vec![
            entry("a", "tfstate", "core"),
            entry("b", "tfstate", "core-extra"),
        ];
        let errs = validate_uniqueness(&entries);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("'core'"));
        assert!(errs[0].contains("'core-extra'"));
    }

    #[test]
    fn validate_uniqueness_allows_substring_that_is_not_kebab_prefix() {
        // 'core' should not collide with 'corextra' — only 'core-extra'.
        let entries = vec![
            entry("a", "tfstate", "core"),
            entry("b", "tfstate", "corextra"),
        ];
        assert!(validate_uniqueness(&entries).is_empty());
    }

    #[test]
    fn validate_uniqueness_allows_prefix_across_kinds() {
        let entries = vec![
            entry("a", "tfstate", "core"),
            entry("b", "cache", "core-extra"),
        ];
        assert!(validate_uniqueness(&entries).is_empty());
    }

    #[test]
    fn namespace_prefix_joins_kind_and_name() {
        assert_eq!(ns("tfstate", "devbox").prefix(), "tfstate/devbox");
    }

    /// Plant a minimal CUE module + stubbed domain registry into
    /// `root` so the project schema's
    /// `import "kolohelios.com/infra/cloudflare-dns/domains:domain"`
    /// resolves when `cue export` runs against a fixture project.cue
    /// here. The tests below don't reference `#KnownHostnames` (no
    /// `serving:` / `deploy:` blocks), so the stub is sufficient.
    /// Local `cargo test` happens to work because cwd is the repo
    /// root (real `cue.mod` is found one walk-up away), but `nix
    /// build`'s sandbox has no enclosing module — this setup makes
    /// the tests pass in both.
    fn plant_test_module(root: &Path) {
        fs::create_dir_all(root.join("cue.mod")).unwrap();
        fs::write(
            root.join("cue.mod/module.cue"),
            "module: \"kolohelios.com\"\nlanguage: version: \"v0.15.1\"\n",
        )
        .unwrap();
        let registry = root.join("infra/cloudflare-dns/domains");
        fs::create_dir_all(&registry).unwrap();
        fs::write(
            registry.join("registry.cue"),
            "package domain\n\n#KnownHostnames: _\n",
        )
        .unwrap();
    }

    #[test]
    fn collect_skips_projects_without_object_storage() {
        let tmp = TempDir::new().unwrap();
        plant_test_module(tmp.path());
        fs::create_dir_all(tmp.path().join("apps/foo")).unwrap();
        fs::write(
            tmp.path().join("apps/foo/project.cue"),
            "package project\n#Project & { name: \"foo\", kind: \"infra\" }\n",
        )
        .unwrap();

        let entries = collect(tmp.path()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn collect_reads_namespaces_from_project_cue() {
        let tmp = TempDir::new().unwrap();
        plant_test_module(tmp.path());
        fs::create_dir_all(tmp.path().join("infra/devbox")).unwrap();
        fs::write(
            tmp.path().join("infra/devbox/project.cue"),
            "package project\n#Project & {\n\
                 name: \"devbox\"\n\
                 kind: \"infra\"\n\
                 objectStorage: namespaces: [{\n\
                     kind: \"tfstate\"\n\
                     name: \"devbox\"\n\
                     purpose: \"remote tfstate\"\n\
                 }]\n\
             }\n",
        )
        .unwrap();

        let entries = collect(tmp.path()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].namespace.kind, "tfstate");
        assert_eq!(entries[0].namespace.name, "devbox");
        assert_eq!(entries[0].namespace.purpose, "remote tfstate");
    }
}

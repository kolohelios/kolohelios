use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use crate::term::{BOLD, DIM, GREEN, RED, RESET, YELLOW};

pub const REFRESH_SNIPPET: &str = "\
REFRESH PROCEDURE

Hover has no API. To refresh the snapshot, sign in at
https://www.hover.com/control_panel and paste this snippet into DevTools:

  (() => {
    const keys = ['name', 'status', 'expiry_date', 'whois_privacy',
                  'locked', 'autorenew', 'nameservers'];
    const data = $('#app').data('props').initialData.data.domains
      .map(d => Object.fromEntries(keys.map(k => [k, d[k]])));
    const blob = new Blob([JSON.stringify(data, null, 2)],
                          { type: 'application/json' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = 'hover-snapshot.json';
    a.click();
  })();

`hover-snapshot.json` lands in ~/Downloads. Pass it via `--input`.

The Blob-download form (rather than a clipboard `copy()` snippet) is
deliberate: clipboard-paste-of-DOM patterns match credential-stealer
signatures (Atomic, StealC) and are blocked by macOS Sequoia's XProtect
clipboard scanner. See \"Things to avoid\" in CLAUDE.md.";

#[derive(Debug, Deserialize)]
struct HoverEntry {
    name: String,
}

/// Shape `cue export` returns for the registry package: a `domains`
/// map keyed by hostname. The value-side is discarded for inventory
/// purposes (the diff cares only about which hostnames exist), so the
/// inner type stays `serde_json::Value`.
#[derive(Debug, Deserialize)]
struct RegistryExport {
    #[serde(default)]
    domains: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct InventoryDiff {
    pub adds: Vec<String>,
    pub removes: Vec<String>,
}

impl InventoryDiff {
    pub fn has_drift(&self) -> bool {
        !self.adds.is_empty() || !self.removes.is_empty()
    }
}

pub fn run(input: &Path, registry_dir: &Path) {
    let hover = match load_snapshot(input) {
        Ok(names) => names,
        Err(e) => die(&format!("could not read snapshot {}: {e}", input.display())),
    };

    let registry = match load_registry(registry_dir) {
        Ok(names) => names,
        Err(e) => die(&format!(
            "could not read registry {}: {e}",
            registry_dir.display()
        )),
    };

    if registry.is_empty() {
        eprintln!(
            "{YELLOW}{BOLD}registry is empty{RESET} ({})",
            registry_dir.display()
        );
        eprintln!(
            "  {DIM}populate the directory with one CUE file per domain (see tools/shaka/schema/domain/domain.cue){RESET}"
        );
        eprintln!(
            "  {DIM}snapshot has {} domain(s); all are reported as adds below{RESET}",
            hover.len()
        );
        eprintln!();
    }

    let diff = diff_inventory(&hover, &registry);
    print_diff(&diff);

    if diff.has_drift() {
        std::process::exit(1);
    }
}

fn load_snapshot(path: &Path) -> Result<BTreeSet<String>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let entries: Vec<HoverEntry> =
        serde_json::from_slice(&bytes).map_err(|e| format!("invalid snapshot JSON: {e}"))?;
    let names: Vec<String> = entries.into_iter().map(|e| e.name).collect();
    let dups = find_duplicates(&names);
    if !dups.is_empty() {
        return Err(format!("duplicate names in snapshot: {}", dups.join(", ")));
    }
    Ok(names.into_iter().collect())
}

fn load_registry(dir: &Path) -> Result<BTreeSet<String>, String> {
    // Missing or .cue-less directory short-circuits to empty registry
    // — the caller surfaces a `registry is empty` warning so the user
    // knows the diff is reporting "everything is an add."
    if !dir.exists() || !has_cue_files(dir) {
        return Ok(BTreeSet::new());
    }

    // Treat the directory as a CUE package; `cue export` walks every
    // non-underscore `.cue` file and merges them into a single value.
    // The aggregate file declares `domains: [string]: schema.#Domain`
    // and each per-domain file adds `domains: "<name>": {...}` —
    // collisions (same key, conflicting values) surface as a CUE
    // error rather than needing manual dedup in Rust. Empty packages
    // export `{}` (no `domains` field), which serde fills with the
    // default empty map via `#[serde(default)]`.
    //
    // `current_dir(dir)` + `.` package path: `cue` rejects absolute
    // directory arguments with "cannot use absolute directory as
    // package path", so we cd in and use the relative `.`. The CUE
    // module root (`cue.mod/module.cue`) is found by walking up from
    // there, which lets `import schema "kolohelios.com/..."` resolve.
    let output = Command::new("cue")
        .arg("export")
        .arg("--out")
        .arg("json")
        .arg(".")
        .current_dir(dir)
        .output()
        .map_err(|e| format!("failed to spawn cue: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        return Err(format!("cue export {}: {detail}", dir.display()));
    }
    let export: RegistryExport = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("could not parse cue export output: {e}"))?;
    Ok(export.domains.into_keys().collect())
}

fn has_cue_files(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| {
            let p = e.path();
            p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("cue")
        })
}

pub fn diff_inventory(hover: &BTreeSet<String>, registry: &BTreeSet<String>) -> InventoryDiff {
    InventoryDiff {
        adds: hover.difference(registry).cloned().collect(),
        removes: registry.difference(hover).cloned().collect(),
    }
}

pub fn find_duplicates(names: &[String]) -> Vec<String> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for name in names {
        *counts.entry(name.as_str()).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .filter(|(_, c)| *c > 1)
        .map(|(name, _)| name.to_string())
        .collect()
}

fn print_diff(diff: &InventoryDiff) {
    if !diff.has_drift() {
        println!("{GREEN}{BOLD}inventory matches{RESET}");
        return;
    }

    println!(
        "{BOLD}inventory drift:{RESET} {} add(s), {} remove(s)",
        diff.adds.len(),
        diff.removes.len()
    );
    for name in &diff.removes {
        println!("  {RED}- {name}{RESET}");
    }
    for name in &diff.adds {
        println!("  {GREEN}+ {name}{RESET}");
    }
}

fn die(msg: &str) -> ! {
    eprintln!("{RED}{BOLD}error:{RESET} {msg}");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn diff_empty_when_sets_equal() {
        let h = names(&["a.com", "b.com"]);
        let r = names(&["a.com", "b.com"]);
        let d = diff_inventory(&h, &r);
        assert!(d.adds.is_empty());
        assert!(d.removes.is_empty());
        assert!(!d.has_drift());
    }

    #[test]
    fn diff_reports_adds_when_hover_has_new() {
        let h = names(&["a.com", "b.com", "c.com"]);
        let r = names(&["a.com"]);
        let d = diff_inventory(&h, &r);
        assert_eq!(d.adds, vec!["b.com", "c.com"]);
        assert!(d.removes.is_empty());
        assert!(d.has_drift());
    }

    #[test]
    fn diff_reports_removes_when_registry_has_extra() {
        let h = names(&["a.com"]);
        let r = names(&["a.com", "b.com", "c.com"]);
        let d = diff_inventory(&h, &r);
        assert!(d.adds.is_empty());
        assert_eq!(d.removes, vec!["b.com", "c.com"]);
        assert!(d.has_drift());
    }

    #[test]
    fn diff_reports_both_adds_and_removes() {
        let h = names(&["a.com", "b.com"]);
        let r = names(&["b.com", "c.com"]);
        let d = diff_inventory(&h, &r);
        assert_eq!(d.adds, vec!["a.com"]);
        assert_eq!(d.removes, vec!["c.com"]);
    }

    #[test]
    fn diff_against_empty_registry_makes_everything_an_add() {
        let h = names(&["a.com", "b.com"]);
        let r: BTreeSet<String> = BTreeSet::new();
        let d = diff_inventory(&h, &r);
        assert_eq!(d.adds, vec!["a.com", "b.com"]);
        assert!(d.removes.is_empty());
    }

    #[test]
    fn diff_results_are_sorted() {
        let h = names(&["z.com", "a.com", "m.com"]);
        let r: BTreeSet<String> = BTreeSet::new();
        let d = diff_inventory(&h, &r);
        assert_eq!(d.adds, vec!["a.com", "m.com", "z.com"]);
    }

    #[test]
    fn find_duplicates_returns_empty_for_unique_names() {
        let v = vec!["a.com".into(), "b.com".into(), "c.com".into()];
        assert!(find_duplicates(&v).is_empty());
    }

    #[test]
    fn find_duplicates_reports_repeated_names() {
        let v = vec![
            "a.com".into(),
            "b.com".into(),
            "a.com".into(),
            "c.com".into(),
            "b.com".into(),
        ];
        assert_eq!(find_duplicates(&v), vec!["a.com", "b.com"]);
    }

    #[test]
    fn find_duplicates_does_not_double_report_triples() {
        let v = vec!["a.com".into(), "a.com".into(), "a.com".into()];
        assert_eq!(find_duplicates(&v), vec!["a.com"]);
    }

    #[test]
    fn find_duplicates_results_are_sorted() {
        let v = vec![
            "z.com".into(),
            "a.com".into(),
            "z.com".into(),
            "a.com".into(),
        ];
        assert_eq!(find_duplicates(&v), vec!["a.com", "z.com"]);
    }
}

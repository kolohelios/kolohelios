//! Export the static game configs (`games/*.cue`) to a single JSON map
//! (`$OUT_DIR/games.json`) that `engine::config` embeds via `include_str!`.
//!
//! CUE is the source of truth; this keeps the Worker free of a CUE runtime
//! while still validating configs against `games/schema.cue` at build. `cue`
//! is on PATH via `worker.extraDevShellPackages` in `project.cue` (locally
//! through direnv, in CI because cf-deploy runs `worker-build` inside the
//! devshell).

use std::path::Path;
use std::process::Command;

fn main() {
    let games_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("games");
    println!("cargo:rerun-if-changed={}", games_dir.display());

    let mut cue_files: Vec<_> = std::fs::read_dir(&games_dir)
        .expect("read games/ directory")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "cue"))
        .collect();
    cue_files.sort();
    for f in &cue_files {
        println!("cargo:rerun-if-changed={}", f.display());
    }
    assert!(!cue_files.is_empty(), "no games/*.cue files found");

    // `-e games` extracts the aggregated `games:` map (id -> #Game) shared
    // across the package's files.
    let output = Command::new("cue")
        .arg("export")
        .args(&cue_files)
        .args(["-e", "games", "--out", "json"])
        .output()
        .expect("run `cue export` (is `cue` on PATH? add it to worker.extraDevShellPackages)");

    assert!(
        output.status.success(),
        "cue export failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let out_file = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR set")).join("games.json");
    std::fs::write(&out_file, &output.stdout).expect("write games.json");
}

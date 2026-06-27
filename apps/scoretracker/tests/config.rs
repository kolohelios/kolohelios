//! Validate the static game configs the same way the build embeds them:
//! every `games/<id>.cue` must `cue vet` against `games/schema.cue`, and the
//! embedded registry must deserialize to the expected games. Fixtures are
//! the real config files, not string literals (repo convention).

use std::path::PathBuf;
use std::process::Command;

fn games_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("games")
}

#[test]
fn every_game_config_vets_against_schema() {
    let dir = games_dir();
    let schema = dir.join("schema.cue");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).expect("read games/ directory") {
        let path = entry.expect("dir entry").path();
        let is_cue = path.extension().is_some_and(|e| e == "cue");
        if !is_cue || path == schema {
            continue;
        }
        let output = Command::new("cue")
            .arg("vet")
            .arg(&schema)
            .arg(&path)
            .output()
            .expect("run `cue vet` (is `cue` on PATH?)");
        assert!(
            output.status.success(),
            "cue vet failed for {}:\n{}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        checked += 1;
    }
    assert!(checked > 0, "expected at least one game config to vet");
}

#[test]
fn registry_loads_the_expected_games() {
    let reg = scoretracker::engine::config::registry();
    for id in ["phase10", "tic", "skipbo"] {
        let game = reg
            .get(id)
            .unwrap_or_else(|| panic!("registry missing {id}"));
        assert_eq!(game.id, id);
        assert!(!game.players.is_empty());
    }
}

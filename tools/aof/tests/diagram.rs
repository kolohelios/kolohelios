use std::path::Path;
use std::process::Command;

use aof::diagram::{self, Area, Tree};

/// True when `bin` answers `--version`. The diagram pipeline shells out
/// to `cue` and `d2`; both are on PATH inside the project devShell (where
/// `just validate` runs), but a bare `cargo test` outside it would fail
/// on spawn. Skip rather than fail in that case.
fn available(bin: &str) -> bool {
    Command::new(bin)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn tree_to_d2_output_renders_to_svg() {
    if !available("d2") {
        eprintln!("skipping: `d2` not on PATH (run inside the devShell)");
        return;
    }

    let tree = Tree {
        roots: vec![Area {
            name: "Example".into(),
            description: None,
            children: vec![Area {
                name: "Work".into(),
                description: None,
                children: vec![],
            }],
        }],
    };
    let source = diagram::tree_to_d2(&tree);
    let svg = diagram::d2_to_svg(&source).expect("d2 must render the source");

    let head = String::from_utf8_lossy(&svg);
    assert!(head.contains("<svg"), "d2 output should be an SVG document");
}

#[test]
fn load_reads_the_areas_fixture() {
    if !available("cue") {
        eprintln!("skipping: `cue` not on PATH (run inside the devShell)");
        return;
    }

    let tree = Tree::load(Path::new("data")).expect("data package must export");
    assert!(!tree.roots.is_empty(), "the fixture has at least one root");
    assert!(
        tree.roots.iter().any(|r| r.name == "Example"),
        "the example fixture's root is named Example"
    );
}

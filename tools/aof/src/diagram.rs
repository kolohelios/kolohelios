//! Turn the areas-of-focus tree into a picture.
//!
//! Two steps, each a thin shell-out:
//!
//! 1. Load the tree by running `cue export <dir>` over the areas CUE
//!    package (schema plus data live in the same package, so the whole
//!    directory is evaluated together) and deserializing the JSON.
//! 2. Emit a D2 source document from the tree and pipe it through
//!    `d2 - -` (read source from stdin, write SVG to stdout). The SVG
//!    bytes are handed to the renderer in `render.rs`.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Deserialize;
use snafu::{ResultExt, Snafu};

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum DiagramError {
    #[snafu(display("could not spawn `cue`: {source} (is `cue` on PATH?)"))]
    SpawnCue { source: std::io::Error },

    #[snafu(display("`cue export {dir}` exited {status}: {stderr}"))]
    CueExport {
        dir: String,
        status: i32,
        stderr: String,
    },

    #[snafu(display("failed to parse `cue export` JSON: {source}"))]
    ParseExport { source: serde_json::Error },

    #[snafu(display("could not spawn `d2`: {source} (is `d2` on PATH?)"))]
    SpawnD2 { source: std::io::Error },

    #[snafu(display("failed to write D2 source to d2 stdin: {source}"))]
    WriteD2Stdin { source: std::io::Error },

    #[snafu(display("failed to wait on `d2`: {source}"))]
    WaitD2 { source: std::io::Error },

    #[snafu(display("`d2` exited {status}: {stderr}"))]
    D2Exit { status: i32, stderr: String },
}

/// A single area-of-focus node. Mirrors `#Area` in `data/schema.cue`:
/// a name, an optional description, and zero or more child areas. The
/// tree shape is enforced by the schema, so there's no cycle handling
/// here by construction.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Area {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub children: Vec<Area>,
}

/// A forest of root areas. `cue export` emits a top-level object whose
/// values are the root `#Area`s — the CUE field names are bookkeeping,
/// not part of the model — so a document with several top-level areas
/// renders as several roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tree {
    pub roots: Vec<Area>,
}

impl Tree {
    /// Load the areas tree by evaluating the CUE package in `dir` (schema
    /// plus data) and deserializing the resulting JSON.
    pub fn load(dir: &Path) -> Result<Self, DiagramError> {
        // `cue` reads a bare relative arg like `data` as a standard-library
        // import path; prefixing with `./` forces it to treat the arg as a
        // filesystem directory. Joining onto "." leaves absolute paths
        // untouched.
        let arg = Path::new(".").join(dir);
        let output = Command::new("cue")
            .arg("export")
            .arg(&arg)
            .output()
            .context(SpawnCueSnafu)?;
        if !output.status.success() {
            return CueExportSnafu {
                dir: arg.display().to_string(),
                status: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            }
            .fail();
        }
        Self::from_export(&output.stdout)
    }

    /// Parse a `cue export` JSON document into a `Tree`. Split out from
    /// `load` so the JSON-to-model mapping is testable without invoking
    /// `cue`.
    pub fn from_export(json: &[u8]) -> Result<Self, DiagramError> {
        let map: BTreeMap<String, Area> = serde_json::from_slice(json).context(ParseExportSnafu)?;
        Ok(Tree {
            roots: map.into_values().collect(),
        })
    }
}

/// Emit a D2 source document for `tree`. Each area becomes a node whose
/// label is the area name; parent-child relationships become `->` edges,
/// which D2's default layout draws as a top-down tree.
///
/// Nodes are given synthetic `nN` identifiers so we never have to worry
/// about escaping area names into valid D2 identifiers — the name rides
/// entirely in the quoted label, and duplicate names across the tree
/// (for example two "Health" areas under different parents) stay
/// distinct.
pub fn tree_to_d2(tree: &Tree) -> String {
    let mut out = String::new();
    let mut next_id = 0usize;
    for root in &tree.roots {
        emit_node(&mut out, root, None, &mut next_id);
    }
    out
}

fn emit_node(out: &mut String, area: &Area, parent_id: Option<&str>, next_id: &mut usize) {
    let id = format!("n{}", *next_id);
    *next_id += 1;

    out.push_str(&id);
    out.push_str(": ");
    out.push_str(&quote_label(&area.name));
    out.push('\n');

    if let Some(parent) = parent_id {
        out.push_str(parent);
        out.push_str(" -> ");
        out.push_str(&id);
        out.push('\n');
    }

    for child in &area.children {
        emit_node(out, child, Some(&id), next_id);
    }
}

/// Wrap `s` in a D2 double-quoted string, escaping backslashes and
/// double quotes so labels containing either stay well-formed.
fn quote_label(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// Render a D2 source document to SVG bytes by piping it through
/// `d2 - -` (source on stdin, SVG on stdout). On success `d2` writes a
/// `success:` line to stderr, which we ignore.
pub fn d2_to_svg(source: &str) -> Result<Vec<u8>, DiagramError> {
    let mut child = Command::new("d2")
        .args(["-", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context(SpawnD2Snafu)?;

    // Write the full source and close stdin before collecting output.
    // `d2` reads its entire input before laying out, so it produces no
    // meaningful stdout until stdin closes — no risk of a pipe deadlock
    // for the diagram sizes we emit.
    child
        .stdin
        .take()
        .expect("stdin was piped above")
        .write_all(source.as_bytes())
        .context(WriteD2StdinSnafu)?;

    let output = child.wait_with_output().context(WaitD2Snafu)?;
    if !output.status.success() {
        return D2ExitSnafu {
            status: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        }
        .fail();
    }
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(name: &str, children: Vec<Area>) -> Area {
        Area {
            name: name.to_string(),
            description: None,
            children,
        }
    }

    #[test]
    fn from_export_maps_top_level_values_to_roots() {
        let json = br#"{"example":{"name":"Example","children":[{"name":"Work"}]}}"#;
        let tree = Tree::from_export(json).expect("valid export parses");
        assert_eq!(tree.roots.len(), 1);
        assert_eq!(tree.roots[0].name, "Example");
        assert_eq!(tree.roots[0].children[0].name, "Work");
    }

    #[test]
    fn from_export_treats_missing_children_as_empty() {
        let json = br#"{"a":{"name":"Leaf"}}"#;
        let tree = Tree::from_export(json).expect("leaf parses");
        assert!(tree.roots[0].children.is_empty());
    }

    #[test]
    fn from_export_supports_multiple_roots() {
        // Two top-level CUE fields => two roots. BTreeMap orders by key,
        // so the output order is deterministic.
        let json = br#"{"b":{"name":"Beta"},"a":{"name":"Alpha"}}"#;
        let tree = Tree::from_export(json).expect("forest parses");
        let names: Vec<&str> = tree.roots.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "Beta"]);
    }

    #[test]
    fn tree_to_d2_emits_a_node_per_area_and_edge_per_child() {
        let tree = Tree {
            roots: vec![area(
                "Example",
                vec![
                    area("Work", vec![]),
                    area("Personal", vec![area("Health", vec![])]),
                ],
            )],
        };
        let d2 = tree_to_d2(&tree);
        // Four nodes (Example, Work, Personal, Health) => four label lines.
        assert_eq!(d2.matches(": \"").count(), 4);
        assert!(d2.contains("n0: \"Example\""));
        // Three edges: Example->Work, Example->Personal, Personal->Health.
        assert_eq!(d2.matches(" -> ").count(), 3);
        // Health hangs off Personal, not the root.
        assert!(d2.contains("n2 -> n3"));
    }

    #[test]
    fn tree_to_d2_keeps_duplicate_names_distinct() {
        // Two "Health" areas under different parents must get distinct
        // node ids so the diagram doesn't collapse them into one.
        let tree = Tree {
            roots: vec![
                area("Body", vec![area("Health", vec![])]),
                area("Mind", vec![area("Health", vec![])]),
            ],
        };
        let d2 = tree_to_d2(&tree);
        assert_eq!(d2.matches("\"Health\"").count(), 2);
        assert!(d2.contains("n0 -> n1"));
        assert!(d2.contains("n2 -> n3"));
    }

    #[test]
    fn quote_label_escapes_quotes_and_backslashes() {
        assert_eq!(quote_label(r#"a"b\c"#), r#""a\"b\\c""#);
    }
}

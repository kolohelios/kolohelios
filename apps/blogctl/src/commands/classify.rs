//! `blogctl classify <slug>` — set classification dimensions on a
//! post.
//!
//! Each `--<dim> value` overwrites the corresponding dimension;
//! `--clear-<dim>` removes it. Unspecified dimensions are left
//! untouched. `--theme a,b` replaces the entire theme list (the only
//! multi-valued dimension in v1).
//!
//! Validates the resulting `Classifications` against the workdir's
//! taxonomy *before* the file write — an invalid value fails fast
//! with an error that lists the allowed options.

use std::fs;
use std::path::PathBuf;

use time::OffsetDateTime;

use crate::classifications::Classifications;
use crate::error::{Error, Result};
use crate::storage::{Repository, Workdir};
use crate::sync::{self, Jj, SyncOptions};

#[derive(Debug, Default)]
pub struct ClassifyArgs {
    pub slug: String,
    pub workdir: PathBuf,
    pub format: Option<String>,
    pub hook: Option<String>,
    pub tone: Option<String>,
    pub audience: Option<String>,
    pub strategic_role: Option<String>,
    pub theme: Vec<String>,
    pub clear_format: bool,
    pub clear_hook: bool,
    pub clear_tone: bool,
    pub clear_audience: bool,
    pub clear_strategic_role: bool,
    pub clear_theme: bool,
    pub no_sync: bool,
}

pub fn run(jj: &dyn Jj, args: ClassifyArgs) -> Result<()> {
    let repo = Repository::open(Workdir::new(&args.workdir))?;
    let (handle, mut post) = repo.load_raw(&args.slug)?;

    let changed_dims = apply(&mut post.metadata.classifications, &args);
    if changed_dims.is_empty() {
        println!("{}: no classification changes", args.slug);
        return Ok(());
    }

    // Validate against the workdir's taxonomy BEFORE touching the
    // filesystem or the jj graph. The error carries the dimension's
    // allowed list so the user sees the legal options on the spot.
    let taxonomy = repo.read_taxonomy()?;
    if let Err(v) = post.metadata.classifications.validate(&taxonomy) {
        return Err(Error::InvalidClassification {
            path: handle.path.clone(),
            dimension: v.dimension,
            value: v.value,
            allowed: v.allowed,
        });
    }

    let config = repo.read_config()?;
    let opts = SyncOptions::from_config(&config.sync, args.no_sync);
    let dim_summary = changed_dims.join(",");
    let message = format!("post({}): classify ({dim_summary})", args.slug);
    let path = handle.path.clone();

    sync::commit_and_push(jj, &args.workdir, &opts, &message, || {
        post.metadata.updated_at = OffsetDateTime::now_utc();
        let rendered = post.render()?;
        fs::write(&path, rendered).map_err(|e| Error::io(&path, e))?;
        Ok(())
    })?;
    println!("{}: classified ({dim_summary})", args.slug);
    Ok(())
}

/// Apply CLI args to `c`. Returns the list of dimension names that
/// actually changed — used both for the commit message and to skip
/// the write entirely when nothing changed.
fn apply(c: &mut Classifications, args: &ClassifyArgs) -> Vec<String> {
    let mut changed = Vec::new();

    apply_single(
        &mut c.format,
        &args.format,
        args.clear_format,
        "format",
        &mut changed,
    );
    apply_single(
        &mut c.hook,
        &args.hook,
        args.clear_hook,
        "hook",
        &mut changed,
    );
    apply_single(
        &mut c.tone,
        &args.tone,
        args.clear_tone,
        "tone",
        &mut changed,
    );
    apply_single(
        &mut c.audience,
        &args.audience,
        args.clear_audience,
        "audience",
        &mut changed,
    );
    apply_single(
        &mut c.strategic_role,
        &args.strategic_role,
        args.clear_strategic_role,
        "strategic_role",
        &mut changed,
    );

    // Theme is multi-valued. --theme a,b REPLACES the list; --clear-theme
    // empties it. They're mutually exclusive in spirit; if both are set
    // we honor --clear-theme last (matches the spirit of an explicit
    // clear).
    if !args.theme.is_empty() && c.theme != args.theme {
        c.theme = args.theme.clone();
        changed.push("theme".into());
    }
    if args.clear_theme && !c.theme.is_empty() {
        c.theme.clear();
        if !changed.contains(&"theme".to_string()) {
            changed.push("theme".into());
        }
    }

    changed
}

fn apply_single(
    field: &mut Option<String>,
    new_value: &Option<String>,
    clear: bool,
    name: &str,
    changed: &mut Vec<String>,
) {
    // Explicit clear takes precedence over a set in the same invocation.
    if clear {
        if field.is_some() {
            *field = None;
            changed.push(name.into());
        }
        return;
    }
    if let Some(v) = new_value {
        if field.as_deref() != Some(v.as_str()) {
            *field = Some(v.clone());
            changed.push(name.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_args() -> ClassifyArgs {
        ClassifyArgs {
            slug: "x".into(),
            workdir: PathBuf::new(),
            ..Default::default()
        }
    }

    #[test]
    fn apply_sets_unset_single_valued_dimension() {
        let mut c = Classifications::default();
        let mut a = empty_args();
        a.format = Some("thesis".into());
        let changed = apply(&mut c, &a);
        assert_eq!(c.format.as_deref(), Some("thesis"));
        assert_eq!(changed, vec!["format"]);
    }

    #[test]
    fn apply_clears_via_clear_flag() {
        let mut c = Classifications {
            tone: Some("sharp".into()),
            ..Default::default()
        };
        let mut a = empty_args();
        a.clear_tone = true;
        let changed = apply(&mut c, &a);
        assert!(c.tone.is_none());
        assert_eq!(changed, vec!["tone"]);
    }

    #[test]
    fn apply_clear_takes_precedence_over_set() {
        let mut c = Classifications::default();
        let mut a = empty_args();
        a.tone = Some("sharp".into());
        a.clear_tone = true;
        let changed = apply(&mut c, &a);
        assert!(c.tone.is_none());
        // Nothing actually changed — it was None and stayed None.
        assert!(changed.is_empty());
    }

    #[test]
    fn apply_theme_replaces_list() {
        let mut c = Classifications {
            theme: vec!["ambiguity".into()],
            ..Default::default()
        };
        let mut a = empty_args();
        a.theme = vec!["delivery".into(), "interfaces".into()];
        let changed = apply(&mut c, &a);
        assert_eq!(c.theme, vec!["delivery", "interfaces"]);
        assert_eq!(changed, vec!["theme"]);
    }

    #[test]
    fn apply_clear_theme_empties_the_list() {
        let mut c = Classifications {
            theme: vec!["ambiguity".into()],
            ..Default::default()
        };
        let mut a = empty_args();
        a.clear_theme = true;
        let changed = apply(&mut c, &a);
        assert!(c.theme.is_empty());
        assert_eq!(changed, vec!["theme"]);
    }

    #[test]
    fn apply_returns_empty_when_nothing_changes() {
        // Args claim to set format=thesis but it's already thesis.
        let mut c = Classifications {
            format: Some("thesis".into()),
            ..Default::default()
        };
        let mut a = empty_args();
        a.format = Some("thesis".into());
        assert!(apply(&mut c, &a).is_empty());
    }

    #[test]
    fn apply_updates_multiple_dimensions_in_one_call() {
        let mut c = Classifications::default();
        let mut a = empty_args();
        a.format = Some("thesis".into());
        a.hook = Some("contradiction".into());
        a.theme = vec!["ambiguity".into()];
        let changed = apply(&mut c, &a);
        // Order matters for the commit message; should be the order
        // dimensions are applied.
        assert_eq!(changed, vec!["format", "hook", "theme"]);
    }
}

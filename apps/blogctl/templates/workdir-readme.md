# blog content workdir

Private workdir for `blogctl`-managed blog post drafts. The Markdown
files under each stage directory are the source of truth; `blogctl`
reads them, moves them across the pipeline, and rewrites their
frontmatter on stage transitions.

## Workflow stages

```
concept → ideation → editing → final-editing → published
                                                  abandoned (terminal)
```

Stage is encoded in two places that must agree: the directory the file
sits in, and the `status:` field in its frontmatter. `blogctl` fails
loudly on any mismatch.

## Post kinds

Every post declares a `kind:` in its frontmatter, mirroring LinkedIn's
split:

- **`post`** — short-form feed content. The default for `blogctl new`.
- **`article`** — long-form with its own permanent URL.

Kind drives prompt selection, exit-criteria thresholds, and optional
per-stage model overrides. Both kinds traverse the same stage pipeline.

```sh
blogctl new "Title" --workdir .                  # kind: post
blogctl new "Title" --workdir . --kind article   # kind: article
```

## Themes

Themes are a narrative-style layer over the kind × stage grid. Each
post declares a `theme:` in its frontmatter, validated against the
`[themes.*]` table in `.blog-os.toml` at create time. `init` seeds two
themes; you can add more by declaring `[themes.<name>]` in the config.

- **`standard`** — default prose. Used when `--theme` is not given.
- **`parable`** — allegorical narrative.

```sh
blogctl new "Title" --workdir .                   # theme: standard (default)
blogctl new "Title" --workdir . --theme parable   # theme: parable
```

Theme drives prompt selection at `blogctl run-stage` time (model and
exit criteria stay theme-agnostic for now).

## Layout

```
<workdir>/
  concepts/
  ideation/
  editing/
  final-editing/
  published/
  abandoned/
  .blog-os.toml
  README.md
  <prompt files>          # e.g. ideation-post-standard.md
```

Prompt files live at the workdir root next to `.blog-os.toml`. They are
loaded by the OpenRouter integration; naming follows the pattern
`<stage>-<kind>-<theme>.md`.

## Version control

This workdir is a colocated `jj` + `git` repository. The convention is
trunk-based — push directly to `main`, no pull requests.

### One-time setup

```sh
gh repo create <name> --private --clone
cd <name>
jj git init --colocate
blogctl init --workdir .
jj describe -m "chore: scaffold workdir"
jj bookmark create main -r @
jj git push --allow-new
```

Two subtleties worth knowing:

- `gh repo create --clone` gives you an empty dir with just `.git/`.
  `jj git init --colocate` on top of an unborn-HEAD git repo works
  fine — push the first commit with `--allow-new` to create the
  remote `main` ref.
- After this, the normal loop is `jj describe`, `jj new`,
  `jj git push`. No PRs — `jj git push --bookmark main` advances
  trunk directly. jj rejects non-fast-forward pushes by default; if
  you ever rewrite `main` (squash, reorder, abandon), you'll need
  `jj git push --allow-backwards` consciously.

## Regenerating this README

This file is generated from a template baked into `blogctl`. Don't edit
it by hand — your edits will be overwritten the next time you
regenerate. To pick up template changes after a `blogctl` upgrade:

```sh
blogctl readme regenerate --workdir .
```

# `blogctl`

Command-line tool for managing Markdown blog post drafts across a linear
workflow. Drafts live in a private `workdir` outside this repo (passed
via `--workdir`); `blogctl` itself ships only as tooling.

## Workflow stages

```
concept → ideation → editing → final-editing → published
                                                  abandoned (terminal)
```

Stage is encoded in two places that must agree: the directory the file
sits in, and the `status:` field in its frontmatter. `blogctl` fails
loudly on any mismatch.

## `workdir` layout

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
  <prompt files>
```

`init` writes the stage directories, `.blog-os.toml`, and a generated
`README.md`. Prompt files (loaded by future OpenRouter integration) live
at the `workdir` root next to the `config`; refresh the README from
`blogctl`'s baked-in template via `blogctl readme regenerate`.

## Commands

Workflow plumbing — moving posts through the editorial pipeline:

```text
blogctl init    --workdir <path>
blogctl new     "Post Title" --workdir <path> [--slug SLUG]
blogctl list    --workdir <path>
blogctl show    <slug> --workdir <path>
blogctl promote <slug> --workdir <path>
blogctl demote  <slug> --workdir <path>
```

Classifying posts and recording per-venue performance:

```text
blogctl classify <slug> --workdir <path> \
  [--format X] [--hook X] [--tone X] [--audience X] \
  [--strategic-role X] [--theme A,B,...] \
  [--clear-<dim>]
blogctl metrics update <slug> --workdir <path> \
  --target <linkedin|blog> \
  --impressions N --reactions N --comments N --reposts N \
  [--sampled-at <RFC3339>]
blogctl metrics show <slug> --workdir <path>
blogctl backfill --workdir <path> [--import <file.json>]
```

Analytics across the corpus — read-only, no mutations:

```text
blogctl analytics summary         --workdir <path> [--target T] [--dimension D] [--json]
blogctl analytics compare <A> <B> --workdir <path> [--target T] [--min-n N] [--json]
blogctl analytics recommendations --workdir <path> [--target T] [--min-n N]
```

`new` writes a fresh `.md` into `concepts/` with a slug derived from the
title (override with `--slug`). `promote`/`demote` move the file across
stage directories and rewrite `status:` and `updated_at:` in the
frontmatter. `published` won't demote without a future `--force` flag;
`abandoned` is reserved for a future explicit transition.

`classify` and `metrics update` take a slug and overwrite the named
dimensions/metrics. `metrics update` requires all four counts —
partial samples are easier to forget than full ones. `backfill` walks
every published post (or imports a JSON batch with `--import`) so the
existing backlog doesn't need one-at-a-time updates.

## Markdown file format

```markdown
---
title: "Example Title"
slug: example-title
kind: post
theme: standard
status: published
created_at: 2026-05-03T00:00:00Z
updated_at: 2026-05-14T00:00:00Z
tags: []
classifications:
  format: thesis
  hook: contradiction
  tone: sharp
  audience: engineering
  strategic_role: career-brand
  theme:
    - ambiguity
    - delivery
targets:
  - name: linkedin
    status: published
    url: https://www.linkedin.com/posts/example
    published_at: 2026-05-08T14:32:00Z
    metrics:
      impressions: 1842
      reactions: 67
      comments: 14
      reposts: 5
      sampled_at: 2026-05-14T00:00:00Z
  - name: blog
    status: planned
---

Draft text here.
```

Timestamps are RFC 3339 UTC. `kind` is one of `post` or `article`;
`theme` is any name declared in `.blog-os.toml`'s `[themes.*]` table
(the binary seeds `standard` and `parable`). The Markdown file is the
source of truth.

`status` tracks the editorial pipeline (where the post sits in
`concept→ideation→editing→final-editing→published`); the directory the
file lives in must agree with it. `targets` is orthogonal: a list of
distribution venues and per-venue state (`planned`, `published`,
`retracted`). `targets` defaults to `[]` and is optional. When a
target's `status` is `published`, both `url` and `published_at` are
required; each venue may appear at most once per post.

`classifications` (also optional, defaults to empty) are structured tag
dimensions used by `analytics`. `targets[].metrics` records the most
recent observed performance numbers for that venue.

## Taxonomy

Classifications are validated against a per-workdir taxonomy declared
in `.blog-os.toml` under `[classifications.<dimension>]` tables:

```toml
[classifications.format]
values = ["parable", "thesis", "essay", "observation",
          "personal-reflection", "framework"]

[classifications.hook]
values = ["proverb", "contradiction", "direct-claim",
          "story-title", "question", "analogy"]

[classifications.tone]
values = ["gentle", "sharp", "vulnerable", "reflective", "provocative"]

[classifications.audience]
values = ["engineering", "product", "leadership", "founders", "general"]

[classifications.strategic_role]
values = ["salal-positioning", "career-brand", "recruiting",
          "writing-practice", "consulting-signal"]

[classifications.theme]
multi = true
values = ["ambiguity", "delivery", "interfaces", "leadership", "ai",
          "engineering-culture", "product", "organizational-psychology"]
```

`init` seeds these tables with the v1 defaults shown above. Adding a
new value is a `.blog-os.toml` edit, not a code change; remove a
dimension's table entirely to stop validating that dimension. Posts
with values outside the declared list refuse to load — `blogctl
doctor` surfaces the offenders.

`multi = false` is the default (single-valued); `multi = true` accepts
a list. `theme` is the only multi-valued dimension in v1.

## Analytics workflow

Once you've classified posts and logged metrics for a few weeks, the
`analytics` commands surface patterns:

```bash
# 1. Draft and publish a post (existing flow).
blogctl new "The only way out is through" --workdir ~/blog-os
# ... edits ...
blogctl promote the-only-way-out-is-through --workdir ~/blog-os
# (repeat through final-editing → published)

# 2. After publishing on LinkedIn, edit `targets[]` in the post's
#    frontmatter to record `url` and `published_at` (by hand for now,
#    or via a future helper).

# 3. After a few days, log the metrics.
blogctl metrics update the-only-way-out-is-through \
  --target linkedin \
  --impressions 1842 --reactions 67 --comments 14 --reposts 5 \
  --workdir ~/blog-os

# 4. Classify the post.
blogctl classify the-only-way-out-is-through \
  --format thesis \
  --hook contradiction \
  --tone sharp \
  --audience engineering \
  --strategic-role career-brand \
  --theme ambiguity,delivery \
  --workdir ~/blog-os

# 5. Refresh metrics weekly. `metrics show` prints the current
#    snapshot per target.
blogctl metrics show the-only-way-out-is-through --workdir ~/blog-os

# 6. Once you have ~15+ measured posts, look at signals:
blogctl analytics summary         --workdir ~/blog-os
blogctl analytics compare format hook --workdir ~/blog-os
blogctl analytics recommendations --workdir ~/blog-os
```

For an existing backlog of published posts with no classifications or
metrics, `blogctl backfill --workdir ~/blog-os` walks them
interactively. With `--import file.json`, it merges a per-slug JSON
batch in one commit — useful when the numbers came out of a CSV
export or a one-off scrape.

### A note on what analytics will (and won't) tell you

Every analytics command operates on a small sample — tens of posts,
not hundreds. The numbers are correlations, not causes; the output is
priors for what to try next, not conclusions. `analytics
recommendations` makes this explicit in every line (`Early signal:`,
`Insufficient data:`, `Stale data:`) and ends every run with a closing
reminder. Treat the other two analytics views the same way: they're
useful for picking the next experiment, not for declaring winners.

## Extension points

- `commands/` — add a new `pub fn run(...)` module and wire it into
  `cli::Command`.
- `storage::Repository` — only place that touches the filesystem.
- `post::Post::parse`/`render` — only place that touches YAML.
- `analytics/` — pure-domain math (percentiles, summary, compare,
  derived metrics, recommendations); the `commands::analytics`
  layer only deals with text/JSON rendering.

OpenRouter calls, prompt loading, historical-consistency checks, and
`Todoist` import are deliberately out of this slice; they slot in alongside
the preceding modules without rewiring.

## Consuming `blogctl`

Published to FlakeHub as `kolohelios/blogctl` on every push to `main`
that touches `apps/blogctl/**` (see `.github/workflows/main.yaml`'s
`build-blogctl` job). Two ways to use it:

- One-shot run:
  ```
  nix run https://flakehub.com/f/kolohelios/blogctl/*.tar.gz -- --help
  ```
- Pin as a flake input from another project:
  ```nix
  inputs.blogctl.url = "https://flakehub.com/f/kolohelios/blogctl/*.tar.gz";
  # then in a devShell:
  packages = [ blogctl.packages.${system}.default ];
  ```

## Development

This project lives in the kolohelios monorepo. Run validation with
`just validate` from inside the project's nix devshell, or `shaka
preflight` from any `cwd` inside one.

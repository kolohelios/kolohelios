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

```text
blogctl init    --workdir <path>
blogctl new     "Post Title" --workdir <path> [--slug SLUG]
blogctl list    --workdir <path>
blogctl show    <slug> --workdir <path>
blogctl promote <slug> --workdir <path>
blogctl demote  <slug> --workdir <path>
```

`new` writes a fresh `.md` into `concepts/` with a slug derived from the
title (override with `--slug`). `promote`/`demote` move the file across
stage directories and rewrite `status:` and `updated_at:` in the
frontmatter. `published` won't demote without a future `--force` flag;
`abandoned` is reserved for a future explicit transition.

## Markdown file format

```markdown
---
title: "Example Title"
slug: example-title
kind: post
theme: standard
status: concept
created_at: 2026-05-03T00:00:00Z
updated_at: 2026-05-03T00:00:00Z
tags: []
todoist_task_id: null
history_checked: false
targets:
  - name: linkedin
    status: published
    url: https://www.linkedin.com/posts/example
    published_at: 2026-05-08T14:32:00Z
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

## Extension points

- `commands/` — add a new `pub fn run(...)` module and wire it into
  `cli::Command`.
- `storage::Repository` — only place that touches the filesystem.
- `post::Post::parse`/`render` — only place that touches YAML.

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

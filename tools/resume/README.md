# resume

Jon Edwards' resume, authored in markdown and rendered to PDF + DOCX via
`pandoc`.

## Canonical profile

`profile.cue` (validated against `schema/profile.cue`) is the single
source for the profile prose shared with the portfolio's about page —
identity, contact, summary, skills, and education. `shaka profile
generate` renders it into:

- this project's `resume.md`, inside the `<!-- BEGIN generated ... -->`
  managed regions (the work-experience sections between them stay
  hand-authored); and
- `apps/kolohelios-portfolio/data/profile.json`, which the portfolio's
  `build-site` reads to render `about.html`.

Edit `profile.cue`, then run `shaka profile generate` and commit. A
repo-level `shaka preflight` check (`shaka profile generate --check`)
fails on drift in either output, regardless of which project a PR
touches. The work-experience bullets are deliberately *not* modelled in
`profile.cue` — the résumé is a tightened distillation of the portfolio's
richer work history, so those stay curated per medium (#785).

## Rendering

```
just build       # produces resume.pdf and resume.docx
just build-pdf   # PDF only (XeLaTeX engine)
just build-docx  # DOCX only (pandoc native)
```

Both outputs are committed to the repo so the latest rendered copy is always a
`git pull` away. `just validate` (and CI) re-renders each `*.md` to a temp
directory and byte-compares against the committed artifact — a stale commit
fails CI. Reproducibility comes from `SOURCE_DATE_EPOCH=0`, which both `pandoc`
and `XeLaTeX` honor for output timestamps. PDF builds also pin `TZ=UTC` and
use a `Lua` filter to derive the PDF identifier from the document rather than
temporary build paths, while preserving the source's header settings.

The flake provides TeX Live packages through Nix, including the fonts and
styles declared in `project.cue`'s `document.texPackages`. Rendering needs no
runtime TeX downloads or pre-existing user cache. This avoids rate limits
from the remote `Tectonic` bundle service on fresh CI runners. After changing renderer or
font packages, regenerate the PDF and check its text and page layout before
committing; `drift-check` still compares the exact output bytes.

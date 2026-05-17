# resume

Jon Edwards' resume, authored in markdown and rendered to PDF + DOCX via
`pandoc`.

## Rendering

```
just build       # produces resume.pdf and resume.docx
just build-pdf   # PDF only (tectonic engine)
just build-docx  # DOCX only (pandoc native)
```

Both outputs are committed to the repo so the latest rendered copy is always a
`git pull` away. `just validate` (and CI) re-renders each `*.md` to a temp
directory and byte-compares against the committed artifact — a stale commit
fails CI. Reproducibility comes from `SOURCE_DATE_EPOCH=0`, which both `pandoc`
and tectonic honor for output timestamps.

// CUE module root for the closure bundled into the packaged `shaka`
// (copied to `$out/share/shaka/cue/cue.mod` by the generated flake).
// Mirrors the repo-root `cue.mod` so the project schema's absolute
// `kolohelios.com/...` imports resolve when shaka runs outside the
// monorepo. See `tools/shaka/src/project/schema_check.rs`.
module: "kolohelios.com"
language: version: "v0.15.1"

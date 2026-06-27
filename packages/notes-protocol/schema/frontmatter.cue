// Canonical shape of note front-matter — the YAML block at the head of a
// note body (title, tags, aliases). It lives beside the `notes-protocol`
// crate, not under a central `schema/` tree, because the Rust
// `FrontMatter` struct in `src/frontmatter.rs` is the other half of the
// same contract: the crate's integration test (`tests/frontmatter.rs`)
// `cue vet`s fixtures against this definition *and* round-trips them
// through the struct, so the two can't drift. Keeping both in one project
// means a schema change and the matching struct change land in one commit.
//
// The definition is open (the trailing `...`): a note may carry keys this
// crate does not model (`created`, `cssclasses`, custom Obsidian keys) and
// still validate, because the Rust `FrontMatter` preserves those keys
// verbatim (`#[serde(flatten)]`) so a vault stays portable to any Markdown
// tool. The two halves agree: unknown keys are accepted-and-preserved, not
// rejected. The known fields are still type-checked — a `title` that is not
// a string, or `tags` that is not a list of strings, fails validation.
// Every field is optional: a note may carry only a title, only tags, or no
// metadata at all.
//
// Vet a YAML front-matter document against it with:
//   cue vet -d '#FrontMatter' schema/frontmatter.cue <doc>.yaml
package frontmatter

#FrontMatter: {
	title?:   string
	tags?:    [...string]
	aliases?: [...string]
	...
}

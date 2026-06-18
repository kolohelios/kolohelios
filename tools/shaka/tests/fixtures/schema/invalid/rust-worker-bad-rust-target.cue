package project

// `extraRustTargets` entries are Rust target triples matching
// `^[a-z0-9_]+(-[a-z0-9_]+)+$` — at least two hyphen-joined segments,
// lowercase. An uppercase / single-segment entry would not be a real
// target the toolchain could fetch std for, so the schema rejects it.
#Project & {
	name: "buzzingo"
	kind: "rust-worker"
	worker: {
		extraRustTargets: ["AArch64-Apple-iOS"]
	}
}

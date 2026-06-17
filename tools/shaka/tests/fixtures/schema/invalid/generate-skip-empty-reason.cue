package project

// A generate.skip entry must carry a non-empty justification (mirrors
// #AuditOverride.justification) — an empty reason is rejected.
#Project & {
	name: "buzzingo"
	kind: "rust-worker"
	worker: {}
	generate: {
		skip: [
			{
				step:   "justfile"
				reason: ""
			},
		]
	}
}

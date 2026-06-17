package project

// rust-worker Cargo workspace that hand-maintains its justfile until shaka
// grows workspace-aware generation (#922): `generate.skip` waives the project
// from `generate-justfiles` (#924).
#Project & {
	name: "buzzingo"
	kind: "rust-worker"
	worker: {}
	generate: {
		skip: [
			{
				step:   "justfile"
				reason: "hand-maintained justfile for the rust-worker workspace until #922"
			},
		]
	}
}

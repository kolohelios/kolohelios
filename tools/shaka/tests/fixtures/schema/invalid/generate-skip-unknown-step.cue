package project

// `step` is a closed enum (`justfile` today); an unknown step is rejected so a
// typo or an un-implemented step can't silently waive nothing.
#Project & {
	name: "buzzingo"
	kind: "rust-worker"
	worker: {}
	generate: {
		skip: [
			{
				step:   "bogus"
				reason: "not a real generator step"
			},
		]
	}
}

package project

// Coverage line.fail is constrained to 0..=100; 150 must fail cue vet.
#Project & {
	name: "bad-editor"
	kind: "wasm-app"
	coverage: {
		line: {
			fail: 150
		}
	}
}

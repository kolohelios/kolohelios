package project

// Coverage is optional on rust-worker, but if declared, the values
// must still be in [0, 100].
#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	coverage: {
		line: {
			fail: 150
		}
	}
}

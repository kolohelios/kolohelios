package project

// Worker app that opts in to coverage on its native unit tests.
#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	worker: {}
	coverage: {
		line: {
			fail: 30
		}
	}
}

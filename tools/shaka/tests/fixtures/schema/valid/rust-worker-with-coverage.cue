package project

// Worker app that opts in to coverage on its native unit tests.
#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	coverage: {
		line: {
			fail: 30
		}
		branch: {
			fail: 50
		}
	}
}

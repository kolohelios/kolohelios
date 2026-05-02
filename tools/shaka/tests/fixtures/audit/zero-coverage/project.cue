package project

#Project & {
	name: "zero-coverage"
	kind: "rust"
	coverage: {
		line: {
			fail: 0
		}
		branch: {
			fail: 20
		}
	}
}

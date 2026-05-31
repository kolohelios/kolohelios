package project

#Project & {
	name: "partial-coverage"
	kind: "rust-cli"
	cli: {
		binaryName: "partial-coverage"
	}
	coverage: {
		line: {
			fail: 30
		}
	}
}

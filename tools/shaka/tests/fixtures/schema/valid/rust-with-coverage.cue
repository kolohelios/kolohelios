package project

#Project & {
	name: "shaka"
	kind: "rust-cli"
	cli: {
		binaryName: "shaka"
	}
	coverage: {
		line: {
			fail: 30
		}
		branch: {
			fail: 50
		}
	}
}

package project

#Project & {
	name: "out-of-range"
	kind: "rust-cli"
	cli: {
		binaryName: "out-of-range"
	}
	coverage: {
		line: {
			fail: 150
		}
		branch: {
			fail: 50
		}
	}
}

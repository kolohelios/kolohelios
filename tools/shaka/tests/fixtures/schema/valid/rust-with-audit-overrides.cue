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
	audit: {
		overrides: [
			{
				rule:          "rust-has-tests"
				severity:      "off"
				justification: "scaffolding-only crate; no behavior to cover yet"
			},
		]
	}
}

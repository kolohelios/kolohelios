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

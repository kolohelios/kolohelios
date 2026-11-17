package project

#Project & {
	name: "example"
	kind: "rust-cli"
	coverage: line: fail: 85
	cli: {
		binaryName: "example"
		buildResources: EXAMPLE_DATA: {
			url: "https://example.org/data"
			hash: "unverified"
		}
	}
}

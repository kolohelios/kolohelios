package project

#Project & {
	name: "example"
	kind: "rust-cli"
	coverage: line: fail: 85
	cli: {
		binaryName: "example"
		buildResources: EXAMPLE_DATA: {
			url: "https://example.org/${builtins.abort}/data"
			hash: "sha256-2GLyFSctsFtnCGcesCfzGrg/3anz1B+MYQLF9QAuJ2g="
		}
	}
}

package project

#Project & {
	name: "example"
	kind: "rust-cli"
	coverage: line: fail: 85
	cli: {
		binaryName: "example"
		buildResources: EXAMPLE_DATA: {
			url: "https://example.org/public-data?v=2&static=true"
			hash: "sha256-2GLyFSctsFtnCGcesCfzGrg/3anz1B+MYQLF9QAuJ2g="
		}
	}
}

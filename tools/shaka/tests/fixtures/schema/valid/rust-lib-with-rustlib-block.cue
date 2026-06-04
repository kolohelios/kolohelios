package project

#Project & {
	name: "notes-protocol"
	kind: "rust-lib"
	coverage: {
		line: {
			fail: 30
		}
	}
	rustLib: {
		description: "notes-protocol — shared wire types"
		extraDevShellPackages: ["wabt"]
	}
}

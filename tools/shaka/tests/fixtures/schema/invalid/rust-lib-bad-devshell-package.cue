package project

// extraDevShellPackages entries are nixpkgs attr names; a value with a
// space can't be one and must fail cue vet.
#Project & {
	name: "bad-devshell-lib"
	kind: "rust-lib"
	coverage: {
		line: {
			fail: 30
		}
	}
	rustLib: {
		extraDevShellPackages: ["has space"]
	}
}

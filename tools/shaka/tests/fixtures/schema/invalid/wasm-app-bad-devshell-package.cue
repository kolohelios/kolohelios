package project

// extraDevShellPackages entries are nixpkgs attr names; a value with a
// space can't be one and must fail cue vet.
#Project & {
	name: "bad-editor"
	kind: "wasm-app"
	wasmApp: {
		extraDevShellPackages: ["has space"]
	}
}

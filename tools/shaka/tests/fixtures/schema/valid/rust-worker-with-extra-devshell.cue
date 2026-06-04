package project

// Worker carrying the flake-shape knobs `shaka project
// generate-flakes` consumes: a `description` override and
// `extraDevShellPackages` on top of the fixed worker base
// (the kolohelios-portfolio shape).
#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	worker: {
		description: "kolohelios portfolio worker"
		extraDevShellPackages: ["tailwindcss", "cue"]
	}
}

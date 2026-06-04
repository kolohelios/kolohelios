package project

// `extraDevShellPackages` entries are nixpkgs attr names matching
// `^[a-zA-Z_][a-zA-Z0-9_-]*$`; an entry with a space (or other stray
// punctuation) would break the generated `pkgs.<attr>` line, so the
// schema rejects it.
#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	worker: {
		extraDevShellPackages: ["tailwind css"]
	}
}

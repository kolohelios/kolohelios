package project

#Project & {
	name: "kolohelios-nix"
	kind: "nix-lib"
	ci: {
		build: {
			filterKey:   "nix-lib"
			jobId:       "nix-lib"
			displayName: "kolohelios-nix"
			// `nix flake check` rather than `nix build` — this is a lib-only
			// flake (lib + formatter + devShells, no derivation output), so
			// the goal is to force eval of every output that flakehub-push
			// then uploads as flake source.
			nixCommand: "check"
			publish: {
				kind:       "flakehub"
				name:       "kolohelios/kolohelios-nix"
				visibility: "public"
				rolling:    true
			}
		}
	}
}

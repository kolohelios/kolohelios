package project

#Project & {
	name: "home"
	kind: "infra"
	ci: {
		build: {
			filterKey:   "home"
			jobId:       "home"
			displayName: "infra/home"
			// Lib-style flake (nixosModules + darwinConfigurations + devShells,
			// no native build output); `nix flake check` forces eval of every
			// output that flakehub-push then uploads as source.
			nixCommand: "check"
			publish: {
				kind:       "flakehub"
				name:       "kolohelios/home"
				visibility: "public"
				rolling:    true
			}
		}
	}
}

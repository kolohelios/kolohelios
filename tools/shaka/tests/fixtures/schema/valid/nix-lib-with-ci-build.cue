package project

#Project & {
	name: "kolohelios-nix"
	kind: "nix-lib"
	ci: {
		build: {
			filterKey:   "nix-lib"
			jobId:       "nix-lib"
			displayName: "kolohelios-nix"
			nixCommand:  "check"
			publish: {
				kind:       "flakehub"
				name:       "kolohelios/kolohelios-nix"
				visibility: "public"
				rolling:    true
			}
		}
	}
}

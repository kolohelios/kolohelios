package project

// rust-worker doesn't get build jobs in main.yaml (its deploy story
// lives in kolohelios-portfolio-deploy.yml); ci.build is not on its
// kind branch, so cue vet should reject the field.
#Project & {
	name: "kolohelios-portfolio"
	kind: "rust-worker"
	ci: {
		build: {
			filterKey:   "portfolio"
			jobId:       "portfolio"
			displayName: "portfolio"
			publish: {
				kind:       "flakehub"
				name:       "kolohelios/portfolio"
				visibility: "public"
				rolling:    true
			}
		}
	}
}

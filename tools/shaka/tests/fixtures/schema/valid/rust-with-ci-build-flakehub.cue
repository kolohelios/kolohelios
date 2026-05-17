package project

#Project & {
	name: "shaka"
	kind: "rust"
	coverage: {
		line: {fail:   30}
		branch: {fail: 20}
	}
	ci: {
		build: {
			filterKey:   "shaka"
			jobId:       "shaka"
			displayName: "shaka"
			publish: {
				kind:       "flakehub"
				name:       "kolohelios/shaka"
				visibility: "public"
				rolling:    true
			}
		}
	}
}

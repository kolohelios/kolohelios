package project

#Project & {
	name: "notes-protocol"
	kind: "rust-lib"
	coverage: {
		line: {
			fail: 30
		}
	}
	ci: {
		build: {
			filterKey:   "notes-protocol"
			jobId:       "notes-protocol"
			displayName: "notes-protocol"
			publish: {
				kind:       "flakehub"
				name:       "kolohelios/notes-protocol"
				visibility: "public"
				rolling:    true
			}
		}
	}
}

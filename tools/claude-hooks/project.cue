package project

#Project & {
	name: "claude-hooks"
	kind: "rust"
	coverage: {
		line: {
			fail: 1
		}
		branch: {
			fail: 0
		}
	}
	ci: {
		build: {
			filterKey:   "claude-hooks"
			jobId:       "claude-hooks"
			displayName: "claude-hooks"
			publish: {
				kind:       "flakehub"
				name:       "kolohelios/claude-hooks"
				visibility: "public"
				rolling:    true
			}
		}
	}
}

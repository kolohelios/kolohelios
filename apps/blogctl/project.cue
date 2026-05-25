package project

#Project & {
	name: "blogctl"
	kind: "rust"
	coverage: {
		// Floors picked from the current measured coverage minus
		// ~7% headroom: as of #442, line is ~92% and branch is ~72%.
		// The branch gap is wider because some error paths and
		// FakeJj fallbacks aren't exercised end-to-end. Bump these
		// gradually as targeted tests fill the gaps.
		line: {
			fail: 85
		}
		branch: {
			fail: 65
		}
	}
	ci: {
		build: {
			filterKey:   "blogctl"
			jobId:       "blogctl"
			displayName: "blogctl"
			publish: {
				kind:       "flakehub"
				name:       "kolohelios/blogctl"
				visibility: "public"
				rolling:    true
			}
			// Notify the blogs-and-posts repo of a fresh publish so its
			// bump-blogctl workflow runs immediately rather than waiting
			// for its daily cron.
			dispatch: [
				{
					repo:      "kolohelios/blogs-and-posts"
					eventType: "blogctl-published"
				},
			]
		}
	}
}

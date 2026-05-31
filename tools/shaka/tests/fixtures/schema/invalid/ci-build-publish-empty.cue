package project

// publish must conform to #PublishFlakehub or #PublishArtifact; an
// empty block satisfies neither (both have required fields without
// defaults) and should fail cue vet.
#Project & {
	name: "shaka"
	kind: "rust-cli"
	cli: {
		binaryName: "shaka"
	}
	coverage: {
		line: {fail:   30}
		branch: {fail: 20}
	}
	ci: {
		build: {
			filterKey:   "shaka"
			jobId:       "shaka"
			displayName: "shaka"
			publish: {}
		}
	}
}

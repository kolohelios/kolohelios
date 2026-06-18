package repopolicy

// A repo with a hand-authored workflow (e.g. an external consumer's
// `just validate` CI gate) declares it so `ci audit-workflows` accounts for it.
#RepoPolicy & {
	defaultBranch: "main"
	merge: {
		rebase:              true
		merge:               false
		squash:              false
		deleteBranchOnMerge: true
		autoMerge:           true
	}
	handAuthoredWorkflows: ["buzzingo-validate.yml"]
}

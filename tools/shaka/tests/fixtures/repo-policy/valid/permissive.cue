package repopolicy

#RepoPolicy & {
	defaultBranch: "main"
	merge: {
		rebase:              true
		merge:               true
		squash:              true
		deleteBranchOnMerge: false
	}
	branchProtection: {
		requiredChecks: []
		strictStatusChecks: false
		allowForcePush:     true
		allowDeletion:      true
	}
	rulesets: {
		requireDeletion:       false
		requireNonFastForward: false
		requireStatusChecks:   false
	}
	security: {
		dependabotAlerts: false
	}
}

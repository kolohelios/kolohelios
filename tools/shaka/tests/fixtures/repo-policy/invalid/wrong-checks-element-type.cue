package repopolicy

#RepoPolicy & {
	defaultBranch: "main"
	merge: {
		rebase:              true
		merge:               false
		squash:              false
		deleteBranchOnMerge: true
	}
	branchProtection: {
		requiredChecks: [123]
		strictStatusChecks: false
		allowForcePush:     false
		allowDeletion:      false
	}
	rulesets: {
		requireDeletion:       true
		requireNonFastForward: true
		requireStatusChecks:   true
	}
	security: {
		dependabotAlerts: true
	}
}

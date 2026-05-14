package repopolicy

#RepoPolicy & {
	defaultBranch: "main"
	merge: {
		rebase:              true
		merge:               false
		squash:              false
		deleteBranchOnMerge: true
		autoMerge:           true
	}
	issues: true
	branchProtection: {
		requiredChecks: ["Gate"]
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

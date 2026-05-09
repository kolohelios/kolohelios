package repopolicy

#RepoPolicy & {
	defaultBranch: "main"
	merge: {
		rebase:              "yes"
		merge:               false
		squash:              false
		deleteBranchOnMerge: true
	}
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

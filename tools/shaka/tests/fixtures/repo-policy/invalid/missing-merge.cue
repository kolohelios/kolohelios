package repopolicy

#RepoPolicy & {
	defaultBranch: "main"
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

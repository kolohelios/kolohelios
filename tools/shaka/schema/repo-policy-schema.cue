package repopolicy

#RepoPolicy: {
	defaultBranch: string & !=""
	merge: {
		rebase:              bool
		merge:               bool
		squash:              bool
		deleteBranchOnMerge: bool
	}
	// Omit any optional field below to opt the audit out of that
	// dimension — useful for personal repos that don't enable issues,
	// branch protection, or rulesets.
	issues?: bool
	branchProtection?: {
		requiredChecks: [...string]
		strictStatusChecks: bool
		allowForcePush:     bool
		allowDeletion:      bool
	}
	rulesets?: {
		requireDeletion:       bool
		requireNonFastForward: bool
		requireStatusChecks:   bool
	}
	security?: {
		dependabotAlerts: bool
	}
}

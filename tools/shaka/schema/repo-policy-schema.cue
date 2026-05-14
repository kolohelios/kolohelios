package repopolicy

#RepoPolicy: {
	defaultBranch: string & !=""
	merge: {
		rebase:              bool
		merge:               bool
		squash:              bool
		deleteBranchOnMerge: bool
		// `allow_auto_merge` on the repo. Required for
		// `gh pr merge --auto` to queue (used by the lock-bump workflows
		// and #57's `shaka repo send` follow-up).
		autoMerge: bool
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

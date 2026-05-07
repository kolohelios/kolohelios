package audit

#AuditConfig & {
	rules: [
		{name: "readme-present", severity: "fail"},
		{name: "gitignore-present", severity: "fail"},
		{name: "rust-has-tests", severity: "fail"},
		{name: "rust-coverage-threshold-nonzero", severity: "fail"},
		{name: "rust-license-dual", severity: "fail"},
		{name: "kolohelios-nix-via-flakehub", severity: "fail"},
		{name: "validate-recipe-meaningful", severity: "fail"},
	]
}

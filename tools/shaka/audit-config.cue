package audit

#AuditConfig & {
	rules: [
		{name: "readme-present", severity: "fail"},
		{name: "gitignore-present", severity: "fail"},
		{name: "rust-has-tests", severity: "fail"},
		{name: "rust-coverage-threshold-nonzero", severity: "fail"},
		{name: "rust-license-dual", severity: "fail"},
		{name: "kolohelios-nix-via-flakehub", severity: "fail"},
		{name: "kolohelios-home-via-flakehub", severity: "fail"},
		{name: "validate-recipe-meaningful", severity: "fail"},
		{name: "rust-cli-package-true-requires-ci-build", severity: "fail"},
		{name: "rust-cli-package-false-requires-override", severity: "fail"},
	]
}

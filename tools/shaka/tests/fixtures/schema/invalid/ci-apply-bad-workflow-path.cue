package project

// reusable_workflow must point at a path inside .github/workflows/;
// a bare relative path should fail the regex constraint.
#Project & {
	name: "cloudflare-dns"
	kind: "infra"
	ci: {
		apply: {
			reusable_workflow: "tf-apply.yml"
		}
	}
}

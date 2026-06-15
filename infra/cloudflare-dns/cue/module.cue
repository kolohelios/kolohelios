// Source of truth for the cloudflare-dns Terraform module. `shaka
// terraform emit` reads this directory and writes `../terraform/module.tf`.
// Never edit the emitted HCL by hand — re-run emit instead. The remote
// backend lives in `../terraform/backend.tf`, generated separately by
// `shaka object-store tfstate emit`.
//
// Schema lives at tools/shaka/schema/terraform-schema.cue.
package terraform

terraform: {
	required_version: ">= 1.0"
	required_providers: cloudflare: {
		source:  "cloudflare/cloudflare"
		version: "~> 5.0"
	}
}

// CLOUDFLARE_API_TOKEN is read from the environment automatically.
// Source it via 1Password: see README.md.
providers: [{
	name: "cloudflare"
}]

variables: [{
	name: "cloudflare_account_id"
	description: "Cloudflare account ID for the zones managed here. Stable and not secret; visible in the dashboard URL. Passed literally rather than looked up because the consumer-token can't list accounts (matches the pattern in infra/cloudflare-tokens; see #311/#313)."
	type: "string"
}]

// The apex is owned by the Worker custom-domain attachment in
// infra/cloudflare-deploy (slice #3 of #186); attaching a Worker custom
// domain installs the routing record itself, and a user-owned CNAME at
// the same hostname conflicts. The zone resource stays here because zones
// are infra/cloudflare-dns's concern; per-app records sit with the app's
// deploy block.
resources: [{
	type: "cloudflare_zone"
	name: "kolohelios_com"
	attributes: {
		account: id: {"$ref": "var.cloudflare_account_id"}
		name: "kolohelios.com"
		type: "full"
	}
}]

// Per-domain expected NS pair and DNSSEC enablement state. This is the
// stable contract `shaka domain check` reads at check-time, decoupling
// the check from TF state file shape. Keep keys aligned with `name`
// fields in `infra/cloudflare-dns/domains/*.cue`.
//
// The value is carried as a single HCL expression via `$ref`: the map
// key `"kolohelios.com"` is not an HCL identifier, and `ns_pair` wraps a
// traversal in a `sort(...)` call — both beyond a plain traversal, now
// that `$ref` accepts arbitrary HCL expressions (#362).
outputs: [{
	name:        "domain_expectations"
	description: "Per-domain expected NS pair and DNSSEC state, keyed by zone name"
	value: {"$ref": #"""
		{
		  "kolohelios.com" = {
		    ns_pair        = sort(cloudflare_zone.kolohelios_com.name_servers)
		    dnssec_enabled = false
		  }
		}
		"""#}
}]

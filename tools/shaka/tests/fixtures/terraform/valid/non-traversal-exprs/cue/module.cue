package terraform

// Exercises non-traversal HCL expressions carried through #Ref (#362):
// function calls, index access, conditionals, and splat. Each `$ref` is
// parsed as a full HCL expression by the emitter rather than restricted
// to a dotted traversal.
outputs: [{
	name:  "ns_sorted"
	value: {"$ref": "sort(cloudflare_zone.k.name_servers)"}
}, {
	name:  "coalesced"
	value: {"$ref": "coalesce(var.label, \"\")"}
}, {
	name:  "first_key"
	value: {"$ref": "var.authorized_keys[0]"}
}, {
	name:  "region"
	value: {"$ref": "var.use_primary ? var.primary : var.secondary"}
}, {
	name:  "all_ipv4"
	value: {"$ref": "linode_instance.devbox.*.ipv4"}
}]

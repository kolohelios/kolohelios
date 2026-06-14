package terraform

// Passes the schema (a non-empty string) but is rejected at emit time:
// the trailing `; rm -rf /` is not a valid HCL expression, so the
// emitter's parser refuses it. This guards against injection through
// `$ref` now that the schema no longer restricts it to dotted
// traversals.
resources: [{
	type: "linode_instance"
	name: "devbox"
	attributes: region: {"$ref": "var.region; rm -rf /"}
}]

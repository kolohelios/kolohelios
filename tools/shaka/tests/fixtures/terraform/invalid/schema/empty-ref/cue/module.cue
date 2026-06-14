package terraform

// An empty `$ref` is rejected at the schema level: #Ref requires a
// non-empty string. Non-empty-but-malformed refs are caught later, at
// emit time (see invalid/emit/bad-ref-syntax).
resources: [{
	type: "linode_instance"
	name: "devbox"
	attributes: region: {"$ref": ""}
}]

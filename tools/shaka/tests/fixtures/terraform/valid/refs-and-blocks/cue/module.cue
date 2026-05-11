package terraform

resources: [{
	type: "linode_volume"
	name: "persist"
	attributes: {
		label:     "devbox-persist"
		region:    {"$ref": "var.region"}
		size:      {"$ref": "var.volume_size"}
		linode_id: {"$ref": "linode_instance.devbox.id"}
	}
	blocks: [{
		type: "lifecycle"
		attributes: prevent_destroy: true
	}]
}]

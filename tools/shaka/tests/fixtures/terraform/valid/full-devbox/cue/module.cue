package terraform

terraform: {
	required_version: ">= 1.0"
	required_providers: linode: {
		source:  "linode/linode"
		version: "~> 2.0"
	}
}

providers: [{
	name: "linode"
	attributes: token: {"$ref": "var.linode_token"}
}]

variables: [
	{
		name:        "linode_token"
		description: "Linode API personal access token"
		type:        "string"
		sensitive:   true
	},
	{
		name:        "region"
		description: "Linode region"
		type:        "string"
		default:     "us-ord"
	},
	{
		name:        "authorized_keys"
		description: "SSH public keys to inject into the instance"
		type:        "list(string)"
		default: []
	},
]

resources: [{
	type: "linode_instance"
	name: "devbox"
	attributes: {
		label:           "devbox"
		region:          {"$ref": "var.region"}
		authorized_keys: {"$ref": "var.authorized_keys"}
		booted:          true
		tags: ["devbox", "nixos"]
	}
}]

outputs: [{
	name:  "instance_id"
	value: {"$ref": "linode_instance.devbox.id"}
}]

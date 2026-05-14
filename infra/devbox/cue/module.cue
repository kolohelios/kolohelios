// Source of truth for the devbox Terraform module. `shaka terraform
// emit` reads this directory and writes `../terraform/module.tf`. Never
// edit the emitted HCL by hand — re-run emit instead.
//
// Schema lives at tools/shaka/schema/terraform-schema.cue.
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
		name:        "instance_type"
		description: "Linode instance plan"
		type:        "string"
		default:     "g6-standard-2"
	},
	{
		name:        "image_id"
		description: "Linode image ID (from uploaded NixOS image, e.g. private/12345678)"
		type:        "string"
	},
	{
		name:        "authorized_keys"
		description: "SSH public keys to inject into the instance"
		type:        "list(string)"
		default: []
	},
	{
		name:        "volume_size"
		description: "Size of the persistent block storage volume in GB"
		type:        "number"
		default:     20
	},
	{
		name:        "root_pass"
		description: "Root password for the Linode instance (required by API, but SSH key auth is used)"
		type:        "string"
		sensitive:   true
	},
]

resources: [
	{
		type: "linode_instance"
		name: "devbox"
		attributes: {
			label:           "devbox"
			region:          {"$ref": "var.region"}
			type:            {"$ref": "var.instance_type"}
			image:           {"$ref": "var.image_id"}
			root_pass:       {"$ref": "var.root_pass"}
			authorized_keys: {"$ref": "var.authorized_keys"}
			booted:          true
			tags: ["devbox", "nixos"]
		}
	},
	{
		// Attached to the instance via linode_id. Linode provider does
		// not have a separate attachment resource — the volume itself
		// tracks it.
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
	},
]

outputs: [
	{
		name:        "devbox_ip"
		description: "Public IPv4 address of the devbox"
		value: {"$ref": "linode_instance.devbox.ipv4"}
	},
	{
		name:        "devbox_id"
		description: "Linode instance ID"
		value: {"$ref": "linode_instance.devbox.id"}
	},
	{
		name:        "volume_filesystem_path"
		description: "Device path of the attached block storage volume"
		value: {"$ref": "linode_volume.persist.filesystem_path"}
	},
]

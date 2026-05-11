package terraform

resources: [{
	type: "linode_instance"
	name: "devbox"
	attributes: {
		label:  "devbox"
		booted: true
	}
}]

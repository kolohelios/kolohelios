package project

#Project & {
	name: "devbox"
	kind: "infra"
	ci: {
		build: {
			filterKey:   "image"
			jobId:       "image"
			displayName: "Linode image"
			attr:        "linodeImage"
			publish: {
				kind:             "artifact"
				name:             "linode-image"
				path:             "result/nixos.img"
				retentionDays:    7
				compressionLevel: 6
			}
		}
	}
}

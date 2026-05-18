package project

#Project & {
	name: "devbox"
	kind: "infra"
	objectStorage: namespaces: [
		{
			kind:    "tfstate"
			name:    "devbox"
			purpose: "Terraform remote state for the devbox Linode infrastructure"
		},
	]
	ci: {
		build: {
			// Historic filter key + job id — predates the project rename
			// from "image" to "devbox". Kept explicit rather than auto-
			// derived from the project name.
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

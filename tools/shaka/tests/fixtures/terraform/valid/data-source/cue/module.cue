package terraform

data_sources: [{
	type: "cloudflare_zone"
	name: "kolohelios_com"
	attributes: filter: {
		name: "kolohelios.com"
	}
}]

resources: [{
	type: "cloudflare_workers_custom_domain"
	name: "kolohelios_portfolio"
	attributes: {
		account_id: {"$ref": "var.cloudflare_account_id"}
		zone_id:    {"$ref": "data.cloudflare_zone.kolohelios_com.zone_id"}
		hostname:   "kolohelios.com"
		service:    "kolohelios-portfolio"
	}
}]

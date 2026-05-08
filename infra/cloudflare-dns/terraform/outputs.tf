# Per-domain expected NS pair and DNSSEC enablement state. This is the
# stable contract `shaka domain check` reads at check-time, decoupling
# the check from TF state file shape. Keep keys aligned with `name`
# fields in `infra/cloudflare-dns/domains/*.cue`.
output "domain_expectations" {
  description = "Per-domain expected NS pair and DNSSEC state, keyed by zone name"
  value = {
    "kolohelios.com" = {
      ns_pair        = sort(cloudflare_zone.kolohelios_com.name_servers)
      dnssec_enabled = false
    }
  }
}

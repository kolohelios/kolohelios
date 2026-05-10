# The Pages project's default subdomain. Stable contract for the apex
# CNAME in `infra/cloudflare-dns` and the `wrangler pages deploy
# --project-name=...` invocation. Hardcoded `name` rather than
# `cloudflare_pages_project.kolohelios_portfolio.subdomain` because that
# attribute can come back empty between the create and the first
# deployment, which would break consumers reading the output.
output "pages_subdomain" {
  description = "Public *.pages.dev subdomain serving the portfolio (target for the apex CNAME)"
  value       = "${cloudflare_pages_project.kolohelios_portfolio.name}.pages.dev"
}

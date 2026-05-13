# Per-token metadata. Token *values* are never emitted — they live only
# in 1Password (via the `onepassword_item` resources in main.tf).
# Consumers source values via `op://...` references; the metadata here
# is the audit/observability surface.
output "provisioned_tokens" {
  description = "Per-token metadata: TF resource id, expires_on, and the 1Password item reference"
  value = {
    "dns-management" = {
      id              = cloudflare_api_token.dns_management.id
      expires_on      = cloudflare_api_token.dns_management.expires_on
      onepassword_uri = "op://${var.onepassword_vault_id}/${onepassword_item.dns_management.title}/password"
    }
    "deploy" = {
      id              = cloudflare_api_token.deploy.id
      expires_on      = cloudflare_api_token.deploy.expires_on
      onepassword_uri = "op://${var.onepassword_vault_id}/${onepassword_item.deploy.title}/password"
    }
  }
}

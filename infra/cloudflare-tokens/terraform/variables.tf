variable "cloudflare_account_id" {
  description = "Cloudflare account ID for account-scoped token resources. Stable and not secret; visible in the dashboard URL. Passed literally rather than looked up because the bootstrap meta-token has only `User:API Tokens:Edit` and can't list accounts."
  type        = string
}

variable "onepassword_vault_id" {
  description = "UUID of the 1Password vault that receives provisioned token values. Vault name: \"Kolohelios Monorepo\"."
  type        = string
}

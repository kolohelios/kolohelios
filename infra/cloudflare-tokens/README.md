# `cloudflare-tokens`

Terraform-managed Cloudflare API tokens. Each token is declared as a CUE
file under `tokens/` (validated by `shaka token cloudflare schema-check`)
and provisioned by the TF module under `terraform/`. Token values land
in 1Password automatically via the `1Password/onepassword` provider; no
hand-paste step.

Canonical issue: #289. Scheduled renewal: #290. Deferred scope: #292
(TF reads CUE registries directly).

## Layout

- `tokens/` — per-token CUE registry, one `#Token` instance per file.
  Validated against `tools/shaka/schema/cloudflare-token.cue`.
- `terraform/main.tf` — Cloudflare and 1Password providers,
  permission-group data sources, token resources.
- `terraform/variables.tf` — `cloudflare_account_id` (literal, copied
  from the dashboard URL — the meta-token's `User:API Tokens:Edit`
  scope can't list accounts, so we don't look it up by name) and
  `onepassword_vault_id` (the canonical "Kolohelios Monorepo" vault,
  UUID `vedq2v6cmtkglnonkenrjneepa`).
- `terraform/outputs.tf` — token metadata (IDs, expiry); never values.

## Bootstrap (one-time setup)

The TF module manages every CF API token *except* the one it needs
to authenticate against CF. That one is hand-minted, narrowly scoped,
and rotated annually.

### 1. Mint the bootstrap meta-token

In the Cloudflare dashboard, navigate to `My Profile → API Tokens →
Create Token → Custom token`:

- **Name**: `Kolohelios Meta Token`
- **Permissions**: `User → API Tokens → Edit` (and only this).
- **Account Resources**: include only the kolohelios account.
- **TTL**: roughly one year out (rotation cadence).
- **Client IP filtering**: optional.

Store the resulting token in the **Kolohelios Monorepo** vault as a
new *Password* item:

- Item title: `Cloudflare Meta Token`
- Store the token value in the `password` field.

Reference path: `op://vedq2v6cmtkglnonkenrjneepa/Cloudflare Meta Token/password`.

The *Password* item type is used (rather than 1Password's *API
Credential* type) because the TF `1password/onepassword` provider only
manages `login`/`password`/`database`/`secure_note` categories — using
*Password* uniformly across hand-stored and TF-provisioned items keeps
references homogeneous (everything is `/password`).

### 2. Provision the 1Password Service Account

In the 1Password web app, **Developer → Service Accounts → Create
Service Account**:

- **Name**: `kolohelios-monorepo-tf`
- **Vault access**: read-write, scoped to the *Kolohelios Monorepo*
  vault only.
- **Token expiration**: 90 days.

One service account covers both local and CI applies — the token is
stored in two places so each context can read it without needing the
other.

Store the SA token in the same vault as a new *Password* item:

- Item title: `1Password Service Account`
- Store the SA token in the `password` field.

Reference path: `op://vedq2v6cmtkglnonkenrjneepa/1Password Service Account/password`.

This is what `.env`'s `OP_SERVICE_ACCOUNT_TOKEN` resolves to under
`op run` — the TF `onepassword` provider reads it from the
environment to authenticate against the vault.

For CI applies, store the same token as a GitHub repository secret so
workflows can export it directly (no interactive `op` session exists
in CI to resolve the `op://` reference):

- GitHub repo **Settings → Secrets and variables → Actions → New
  repository secret**.
- **Name**: `OP_SERVICE_ACCOUNT_TOKEN`.
- **Value**: the SA token (same value as the 1Password item's
  `password` field).

CI workflows then declare:

```yaml
env:
  OP_SERVICE_ACCOUNT_TOKEN: ${{ secrets.OP_SERVICE_ACCOUNT_TOKEN }}
```

and `op run --env-file=.env -- just apply` works without an
interactive `op` session: the CLI authenticates against the vault
using the SA token already in the environment, resolves the
remaining `op://` references (CF tokens, S3 credentials) from the
vault, and exports them for the wrapped command.

**Rotation**: when the SA token is regenerated (every 90 days, or
ad-hoc if compromised), update both stores — the 1Password vault item
and the GitHub secret. The two stores aren't auto-synced; #290 will
automate the rotation.

### 3. Author `.env`

```
cp .env.example .env
```

`.env` is `gitignored`. Every command below is invoked through `op run
--env-file=.env --` so secrets resolve at the moment of use and never
sit on disk in plaintext.

## Validate, plan, apply

Inside the project devshell (entered automatically by `direnv`):

```
just validate                                    # tofu validate, fmt, whitespace
op run --env-file=.env -- just plan              # remote-state init + plan
op run --env-file=.env -- just apply             # apply
```

`just plan` runs `tofu init -input=false` against the remote backend, so
the AWS_* references in `.env` must be in place too.

## Adding a token

Until #292 lands and TF reads CUE directly, adding a token is two-file:

1. Create `tokens/<name>.cue` declaring the `#Token`. Schema-check
   (`shaka token cloudflare schema-check`) gates the shape.
2. Add a matching `cloudflare_api_token.<name>` resource and an
   `onepassword_item.<name>` resource to `terraform/main.tf` (or a
   sibling `.tf` file). Resource names must match the CUE `name`.

Run `just validate` to make sure the schema and TF agree, then `op run
--env-file=.env -- just plan` to confirm the diff before applying.

## Rotation

Tokens carry an `expires_on` field; CF revokes them at the deadline.
Keep the dates in `tokens/*.cue` and `terraform/main.tf` in sync and
advance them before rotating. The Terraform resources reject past dates
at plan time: Cloudflare can otherwise accept a newly created token that
is already expired, and a successful apply does not prove it can deploy.
Wrangler dry-runs do not validate live API credentials either (#1134).

Tokens opt into automatic renewal with a `renewal` policy in their CUE
entry: `valid_days: 90`, `renew_before_days: 14`, and a `secret_ref` for
the existing 1Password item. The bootstrap meta-token and the 1Password
service-account token are not managed by this renewal policy.

`.github/workflows/renew-cloudflare-tokens.yaml` runs daily and supports
manual dispatch. It invokes `shaka token cloudflare renew`, which advances
both CUE and TF expiry declarations for due tokens and opens one bot PR.
An existing open renewal PR is left alone; CI failures on that PR need
attention before the current credentials expire. Auto-merge queues the PR
behind the normal gate. The scheduler needs no Cloudflare or 1Password
credentials, only the existing GitHub App token.

After merge, the existing token-apply workflow replaces credentials when
their expiry changes and updates the same 1Password items. Terraform uses
create-before-destroy; state remains in the remote backend, never in a
commit. Post-apply verification reads the fresh values from 1Password and
checks Cloudflare's live token-verification endpoint for active status
and the expected expiry. It never prints token values. Failures leave the
apply workflow red rather than treating a successful apply as sufficient.

Preview a renewal locally without credentials or file changes:

```sh
shaka token cloudflare renew --dry-run
shaka token cloudflare renew --dry-run --now 2027-01-27T00:00:00Z
```

Without `--dry-run`, the command edits the declarations locally. For an
emergency manual rotation, advance the expiry first, then run:

```sh
op run --env-file=.env -- just apply -replace=cloudflare_api_token.<name>
shaka token cloudflare verify --registry-dir tokens
```

Consumers need only re-run `op run` to read the new value. Keep the
GitHub App, bootstrap meta-token, and 1Password service-account credentials
valid separately; their failure will also block automated renewal.

## Remote state

State is held in the shared `kolohelios` `Linode` Object Storage bucket
under `tfstate/cloudflare-tokens/terraform.tfstate`. The backend is
declared by `terraform/backend.tf`, which is generated by `shaka
object-store tfstate emit --module infra/cloudflare-tokens/terraform`
(re-run if the namespace changes; do not edit by hand).

S3-compatible credentials for the bucket come from the same
`shaka object-store init` flow `infra/devbox` and `infra/cloudflare-dns`
use; references live in `.env.example`.

## Why not `cloudflare-dns`-style hand-minted tokens?

Solo developer or not, four-plus tokens accumulate fast (DNS, Workers
per slice #3 / #189, `blogctl` secrets later) and dashboard-state
permission grants leave no record outside the dashboard. Doing it once,
properly, beats clicking through token `CRUD` per project. See #289 for
the full motivation.

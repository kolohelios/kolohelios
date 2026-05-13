# Hover → Cloudflare migration

Procedure for moving a domain from Hover-default `nameservers` to
Cloudflare. Hover has no API or Terraform provider, so the registrar-
side parts are manual and one-time per domain. Document once, follow
exactly.

Used by:

- Slice #1 of #186 (`kolohelios.com` — done; this `runbook` is
  forensically reconstructed from the live walk-through).
- #197 — the 51-domain bulk move; one pass through this `runbook` per
  domain.

## Prerequisites

- **`infra/cloudflare-tokens` applied.** The DNS Management token is in
  `op://vedq2v6cmtkglnonkenrjneepa/Cloudflare DNS Management
  Token/password`. See that project's `README.md` for the
  bootstrap (meta-token + 1Password Service Account).
- **`Linode` object-store bucket exists.** `shaka object-store init`
  ran once; the bucket holds TF remote state for every infra project.
- **1Password unlocked.** Either run `eval $(op signin)` in the
  shell, or enable the 1Password 8 desktop app's "Integrate with
  1Password CLI" toggle so `op run` resolves references via the app's
  unlocked session.
- **The domain is registered at Hover** (or whoever owns it is ready
  to do the NS swap when prompted).

## Step 0 — Pre-flight: what state already exists?

Past or concurrent work may have left state behind. **Check before
touching TF.** Cold-applying without this step is how `kolohelios.com`
hit "zone already exists" and a clipboard-paste-of-zone-id followed.

**Does the CF zone already exist?**

```
CLOUDFLARE_API_TOKEN="op://Kolohelios Monorepo/Cloudflare DNS Management Token/password" \
  op run -- bash -c 'curl -s -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" "https://api.cloudflare.com/client/v4/zones?name=<domain>" | jq -r ".result[].id"'
```

A non-empty result means the zone is already in CF. You'll
`tofu import` in step 3 rather than create. Note the zone ID.

**What does Hover currently advertise?**

Sign in to hover.com → the domain's page → check the **`Nameservers`**
section. If it already shows two `*.ns.cloudflare.com` `hostnames`
from a previous migration, the registrar side is already done. You
may still need to reconcile TF state (step 3 import path).

## Step 1 — Declare the domain in the registry

Add `infra/cloudflare-dns/domains/<domain-with-hyphens>.cue` (dots
become hyphens — `kolohelios.com` → `kolohelios-com.cue`):

```cue
package domain

import schema "kolohelios.com/tools/shaka/schema/domain"

domains: "<domain>": schema.#Domain & {
	disposition:    "<role>"
	nameservers:    "cloudflare"
	dnssec_enabled: false
}
```

The key (`"<domain>"`) is the `hostname`; the `domains` map is merged
across every file in the registry package, so this single entry adds
to the aggregate. `disposition` is one of the values declared in
`tools/shaka/schema/domain/domain.cue` (`portfolio-canonical`,
`portfolio-alias`, `personal-alt`, `product-reserve`, `park`,
`let-expire`). `shaka domain schema-check` (in `shaka preflight`)
gates the shape.

DNSSEC stays off here — flip in step 7 once the zone is otherwise
stable.

## Step 2 — Add the TF zone resource

In `terraform/main.tf`, add:

```hcl
resource "cloudflare_zone" "<resource_name>" {
  account = {
    id = var.cloudflare_account_id
  }
  name = "<domain>"
  type = "full"
}
```

Resource name = CUE `name` with dots rewritten to underscores:
`kolohelios.com` → `kolohelios_com`. Matches the deterministic mapping
#292 will eventually auto-emit.

Then in `terraform/outputs.tf`, extend the `domain_expectations` map
with a new key for the zone:

```hcl
"<domain>" = {
  ns_pair        = sort(cloudflare_zone.<resource_name>.name_servers)
  dnssec_enabled = false
}
```

CUE → TF sync is manual until #292 auto-syncs.

## Step 3 — Get the zone into TF state

Two paths depending on step 0:

**Zone doesn't exist in CF yet** — create it:

```
op run --env-file=.env -- just plan       # expect: 1 to add
op run --env-file=.env -- just apply
```

**Zone already exists in CF** — import it:

```
op run --env-file=.env -- bash -c "cd terraform && tofu import cloudflare_zone.<resource_name> <zone-id>"
op run --env-file=.env -- just plan       # check for drift
```

If the post-import plan shows drift, you have two options:

- **Update `main.tf` to match reality** — preserves existing zone
  settings the imported zone has but the repo doesn't declare.
- **Apply the repo's view over CF** — only safe if you know the
  divergence isn't load-bearing.

For a freshly discovered legacy zone with default settings, "no
changes" after import is the common case (verified for
`kolohelios.com`).

## Step 4 — Capture the CF NS pair

```
op run --env-file=.env -- bash -c 'cd terraform && tofu output -json domain_expectations | jq -r ".\"<domain>\".ns_pair[]"'
```

Two `*.ns.cloudflare.com` `hostnames` print. CF assigns this pair at
zone creation; it's stable for the life of the zone. Save them — they
go into Hover next.

## Step 5 — NS swap at Hover

Manual, one-time per domain. Hover has no API; this is a UI walk:

1. Sign in to hover.com.
2. **Domains → `<domain>`**.
3. **`Nameservers`** section → **Edit**.
4. Replace the two existing values with the CF pair from step 4.
5. **Save**.

Hover commits the change to the registry's parent-zone update queue.
`TTLs` at `.com` (and other `TLDs`) control how fast resolvers see it.

## Step 6 — Verify the delegation

Two complementary queries:

**Recursive resolver** — data lands in ANSWER, so `+short` works:

```
dig +short NS <domain> @1.1.1.1
```

Should print the two `*.ns.cloudflare.com` `hostnames` once recursive
resolvers have refreshed.

**TLD direct** — bypasses recursive caches; this is the registry-of-
record's view:

```
dig NS <domain> @a.gtld-servers.net
```

Look at the `AUTHORITY SECTION` near the bottom — that's where
`TLD` root servers return delegation NS records. **`+short` doesn't
print AUTHORITY**, which is why a `dig +short NS … @TLD` query
returns empty even when the delegation is correct.

`TLD` NS records cache for ~48 hours (TTL 172800). The TLD-direct
query is ground truth; the recursive query reflects what the rest of
the internet actually sees.

## Step 7 — Enable DNSSEC (deferred per domain)

DNSSEC requires registrar-side coordination (a DS record entered at
Hover) and is deferred until the zone is otherwise stable. When ready:

1. Flip the CUE: `dnssec_enabled: true` in `domains/<domain>.cue`,
   and update `terraform/outputs.tf` to match.
2. Apply TF. CF starts signing the zone and the dashboard exposes
   the DS values.
3. In the CF dashboard: **Zone → DNS → Settings → DNSSEC** → enable
   → copy the DS record (`KeyTag`, `Algorithm`, `DigestType`,
   `Digest`).
4. In Hover: **Domains → `<domain>`** — find the DNSSEC section
   (often near `nameservers`; UI shifts periodically). Enter the
   four DS values.
5. Wait for parent-zone propagation. Verify with:

   ```
   dig DS <domain> @a.gtld-servers.net
   ```

   The DS record should appear in the ANSWER (`TLDs` answer DS in
   ANSWER, not AUTHORITY — different shape from NS).

## Lessons baked in

This guide's shape is informed by surprises from the
`kolohelios.com` walk-through:

- **Check CF and Hover state before touching TF.** A long-abandoned
  repo (`kolohelios_home`) left `kolohelios.com`'s zone live in CF
  *and* Hover already pointing at the CF pair. Cold-applying TF
  crashed with "zone already exists"; the fix was `tofu import` plus
  a no-op plan. Step 0 catches this preemptively.
- **`dig +short NS … @TLD` returns empty even when delegation is
  correct.** `TLD` root servers answer NS queries in AUTHORITY (the
  delegation), not ANSWER, and `+short` only prints ANSWER. Step 6
  uses two queries that show data in the right places.
- **Account-listing data sources need User-level token permissions
  no least-privilege token grants.** `data "cloudflare_accounts"`
  was removed in #311 / PR #312 and #313 / PR #314. Account ID is
  passed literally via `var.cloudflare_account_id` instead. Doesn't
  affect this guide's flow but explains why the variable exists.

## Inventory refresh (separate concern)

Hover has no API. Mass discovery — "what do I own at Hover?" — is its
own thing, with the safe Blob-download DevTools snippet documented
in:

```
shaka domain inventory --help
```

Run that for the snippet and procedure. The procedure above assumes
you already know which domain you're moving; inventory is for picking
candidates and reconciling against `domains/`.

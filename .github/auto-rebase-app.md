# kolohelios-bot GitHub App

The **kolohelios-bot** GitHub App backs repo automation that needs to
push or open PRs and have the resulting CI re-trigger normally. The
App's settings, secrets, and install scope are recorded here so they can
be reproduced (after rotation, recreation, or migration to another
account).

Current consumers:

- `auto-rebase-prs.yaml` — force-pushes rebased PR branches and posts
  the `auto-rebase` commit status when `main` moves.
- `bump-kolohelios-nix.yaml` — pushes the `bot/bump-kolohelios-nix`
  branch and opens (or updates) the lockstep daily bump PR.

## Why a GitHub App, not `GITHUB_TOKEN`

Pushes authenticated via the workflow's default `GITHUB_TOKEN` **do not
re-trigger CI** on the branch they push to. Both consumers need this:
auto-rebase force-pushes onto open PRs and the bump workflow pushes
fresher locks onto its own PR, and in either case we want the PR's
normal CI to re-run against the new tip — otherwise the PR shows green
checks against stale state. A GitHub App token is a separate identity,
so its pushes are treated as ordinary contributor pushes and CI runs.

The App also keeps bot commits' `committer` field as
`kolohelios-bot[bot]` rather than `github-actions[bot]`, which makes the
provenance obvious in `git log`.

## App settings

Reproduce these on https://github.com/settings/apps/new (personal account)
if the App needs to be recreated.

### Basic info

| Field | Value |
| --- | --- |
| GitHub App name | `kolohelios-bot` |
| Description | Automation account for the kolohelios monorepo. Rebases open PRs onto main when main moves and posts `auto-rebase` commit statuses. May expand to other repo automation that needs to act outside personal credentials. |
| Homepage URL | `https://github.com/kolohelios/kolohelios` *(placeholder; required field, not used at runtime)* |

### Identifying and authorizing users

All off — we authenticate as the App, not on behalf of users.

- Callback URL: blank
- Request user authorization (OAuth) during installation: **off**
- Enable Device Flow: **off**

### Post installation

- Setup URL: blank
- Redirect on update: **off**

### Webhook

- Active: **off**

We don't subscribe to App events; the trigger is `push: main` in the
workflow itself. Leaving Webhook → Active off avoids GitHub asking for a
Webhook URL.

### Permissions

**Repository permissions:**

| Permission | Access | Why |
| --- | --- | --- |
| Contents | Read & write | Force-push rebased branches and bump-lock branches |
| Commit statuses | Read & write | Post the `auto-rebase` status |
| Pull requests | Read & write | List open PRs against main; open the daily `bot/bump-kolohelios-nix` PR |
| Metadata | Read | Mandatory for all Apps |

Everything else: **No access**.

> **Changing permissions later requires installation acceptance.** Adding
> or widening a permission on the App's settings page mints a new
> "review request" on the installation
> (https://github.com/settings/installations → kolohelios-bot). Until you
> click through and accept, freshly-minted installation tokens still
> carry the *old* scope and any workflow that depends on the new
> permission will fail with `Resource not accessible by integration`.

**Organization permissions:** all **No access**.
**Account permissions:** all **No access**.

### Subscribe to events

None (webhook is off).

### Where can this app be installed?

**Only on this account** — locks installation to the `kolohelios` user
account.

## Repo configuration

After creating the App, install it on `kolohelios/kolohelios` only and
populate two repo-level entries:

| Kind | Name | Value |
| --- | --- | --- |
| Variable | `KOLOHELIOS_BOT_APP_ID` | The numeric App ID (top of the App's settings page) |
| Secret | `KOLOHELIOS_BOT_APP_PRIVATE_KEY` | The full `.pem` contents downloaded from "Generate a private key" — including `-----BEGIN/END PRIVATE KEY-----` lines |

Both are referenced as `${{ vars.KOLOHELIOS_BOT_APP_ID }}` and
`${{ secrets.KOLOHELIOS_BOT_APP_PRIVATE_KEY }}` in the consuming
workflows.

## Branch protection

**Do not** mark the `auto-rebase` commit status as a required check.
The check only appears on a PR after `main` moves while the PR is open
— making it required would block every PR that opens against the
current tip of main, since no auto-rebase has run yet for that PR. The
status is informational: a failing `auto-rebase` flags a conflict the
author needs to resolve, but doesn't itself gate merge.

## Rotation

Private keys can be re-generated from the App's settings page at any
time. After regenerating, update the `KOLOHELIOS_BOT_APP_PRIVATE_KEY`
secret in repo settings and revoke the old key. Workflow runs in flight
will fail on token mint after revocation; new runs use the new key.

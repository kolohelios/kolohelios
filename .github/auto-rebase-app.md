# `kolohelios-bot` GitHub App

The **`kolohelios-bot`** GitHub App backs repo automation that needs to
push or open PRs and have the resulting CI re-trigger normally. The
App's settings, secrets, and install scope are recorded here so they can
be reproduced (after rotation, recreation, or migration to another
account).

The App's consumers are workflows that push or open PRs and need the
resulting CI to re-trigger normally — auto-rebasing PRs when `main`
moves, or bumping flake inputs daily. They live across every repo the
App is installed on. To list the current set:

```sh
gh search code --owner kolohelios 'KOLOHELIOS_BOT_APP_ID' --extension yaml \
  --json repository,path \
  --jq '.[] | "\(.repository.nameWithOwner): \(.path)"' | sort -u
```

Every match needs the same `KOLOHELIOS_BOT_APP_ID` variable and
`KOLOHELIOS_BOT_APP_PRIVATE_KEY` secret on its repo — see
[Repo configuration](#repo-configuration). The discovery query is the
single source of truth: don't keep a hand-maintained list.

## Why a GitHub App, not `GITHUB_TOKEN`

Pushes authenticated via the workflow's default `GITHUB_TOKEN` **do not
re-trigger CI** on the branch they push to. Every consumer needs this:
auto-rebase force-pushes onto open PRs and the bumpers push fresher
locks onto their own PR, and in either case the normal CI for the PR
must re-run against the new tip — otherwise the PR shows green checks
against stale state. A GitHub App token is a separate identity, so its
pushes are treated as ordinary contributor pushes and CI runs.

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

All off — the bot authenticates as the App, not on behalf of users.

- Callback URL: blank
- Request user authorization (`OAuth`) during installation: **off**
- Enable Device Flow: **off**

### Post installation

- Setup URL: blank
- Redirect on update: **off**

### Webhook

- Active: **off**

No App events subscribed; the trigger is `push: main` in the workflow
itself. Leaving Webhook → Active off avoids GitHub asking for a Webhook
URL.

### Permissions

**Repository permissions:**

| Permission | Access | Why |
| --- | --- | --- |
| Contents | Read & write | Force-push rebased branches and bump-lock branches |
| Commit statuses | Read & write | Post the `auto-rebase` status |
| Pull requests | Read & write | List open PRs against main; open and update the bumpers' `bot/bump-*` PRs |
| Metadata | Read | Mandatory for all Apps |

Everything else: **no access**.

> **Changing permissions later requires installation acceptance.** Adding
> or widening a permission on the App's settings page mints a new
> "review request" on the installation
> (https://github.com/settings/installations → `kolohelios-bot`). Until
> you click through and accept, freshly-minted installation tokens still
> carry the *old* scope and any workflow that depends on the new
> permission fails with `Resource not accessible by integration`.

**Organization permissions:** all **No access**.
**Account permissions:** all **No access**.

### Subscribe to events

None (webhook is off).

### Where can this app be installed?

**Only on this account** — locks installation to the `kolohelios` user
account.

## Repo configuration

For each repo with an App-authenticated workflow, two things must be
configured: App access (so the App can mint a token for that repo) and
credentials (the variable + secret the workflow reads). Same procedure
for the first consumer and every consumer added after.

### App access

Open <https://github.com/settings/installations> → Configure
`kolohelios-bot` → **Repository access** → **Only select repositories**
→ add the repo → **Save**. The App's "Where can this app be installed?"
setting (Only on this account) restricts target repos to those owned by
the kolohelios account.

### Credentials

| Kind | Name | Value |
| --- | --- | --- |
| Variable | `KOLOHELIOS_BOT_APP_ID` | The numeric App ID (top of the App's settings page; not sensitive) |
| Secret | `KOLOHELIOS_BOT_APP_PRIVATE_KEY` | Full `.pem` contents — including the `-----BEGIN/END PRIVATE KEY-----` lines |

Both are referenced as `${{ vars.KOLOHELIOS_BOT_APP_ID }}` and
`${{ secrets.KOLOHELIOS_BOT_APP_PRIVATE_KEY }}` in the consuming
workflows. The PEM is stored canonically in 1Password
(`Kolohelios Monorepo` vault, id `vedq2v6cmtkglnonkenrjneepa`) as the
document `kolohelios-bot GitHub App private key`.

Concrete commands to wire a new consumer repo (replace `<repo>` and
`<APP_ID>`; the App ID is the same number for every consumer repo,
since they all share one App):

```sh
# Variable: the App ID is public, no secret handling needed.
gh variable set KOLOHELIOS_BOT_APP_ID -R <repo> -b '<APP_ID>'

# Secret: pipe the PEM straight from 1Password into the gh secret set.
op document get 'kolohelios-bot GitHub App private key' \
  --vault 'Kolohelios Monorepo' \
| gh secret set KOLOHELIOS_BOT_APP_PRIVATE_KEY -R <repo>
```

If the 1Password item doesn't exist yet, or you want to rotate the
key, follow [Rotation](#rotation) — that procedure both regenerates
the PEM and refreshes the 1Password item.

## Branch protection

**Do not** mark the `auto-rebase` commit status as a required check.
The check only appears on a PR after `main` moves while the PR is open
— making it required would block every PR that opens against the
current tip of main, since no auto-rebase has run yet for that PR. The
status is informational: a failing `auto-rebase` flags a conflict the
author needs to resolve, but doesn't itself gate merge.

## Rotation

> **The PEM lives canonically in 1Password** — `Kolohelios Monorepo`
> vault (id `vedq2v6cmtkglnonkenrjneepa`), document
> `kolohelios-bot GitHub App private key`. The local download from the
> App's settings page is ephemeral material: stash it in 1Password
> (replacing the existing item), push it to every consumer repo's
> Actions secrets, then delete the local file. Don't search
> `~/Downloads`, `~/.ssh`, or anywhere else for an old copy — if the
> 1Password item is gone, the procedure below regenerates from the App
> settings page and re-creates it.

### Steps

1. **Generate a new key** at
   <https://github.com/settings/apps/kolohelios-bot> → **Private keys**
   → **Generate a private key**. Save the resulting `.pem` to a temp
   path:
   ```sh
   PEM=~/Downloads/kolohelios-bot.$(date +%Y-%m-%d).private-key.pem
   ```

2. **Save (or replace) the 1Password item** so it stays the canonical
   copy. First rotation creates the document; subsequent rotations
   replace its contents:
   ```sh
   # First time only — create the document:
   op document create "$PEM" \
     --vault 'Kolohelios Monorepo' \
     --title 'kolohelios-bot GitHub App private key'
   # Subsequent rotations — replace the file contents:
   op document edit 'kolohelios-bot GitHub App private key' "$PEM" \
     --vault 'Kolohelios Monorepo'
   ```
   (1Password GUI works too — drop the new `.pem` onto the existing
   document, or create it the first time.)

3. **Update the secret on every consumer repo** in lockstep — same
   key, every repo, single source of truth:
   ```sh
   mapfile -t REPOS < <(
     gh search code --owner kolohelios 'KOLOHELIOS_BOT_APP_ID' --extension yaml \
       --json repository --jq '.[].repository.nameWithOwner' | sort -u
   )
   for repo in "${REPOS[@]}"; do
     gh secret set KOLOHELIOS_BOT_APP_PRIVATE_KEY -R "$repo" < "$PEM"
   done
   ```

4. **Revoke the old key** on the App settings page. Workflow runs in
   flight fail on token mint after revocation; new runs use the new
   key.

5. **Delete the local `.pem`** — the canonical copy is in 1Password
   and the deployed copies are in Actions secrets, so the temp
   download has no further job:
   ```sh
   rm "$PEM"
   ```

6. **Verify each consumer is healthy.** Trigger every
   `workflow_dispatch`-able workflow found by the discovery query;
   non-dispatchable ones (for example, `auto-rebase-prs.yaml`, which
   only fires on `push: main`) verify themselves the next time their
   trigger fires:
   ```sh
   gh search code --owner kolohelios 'KOLOHELIOS_BOT_APP_ID' --extension yaml \
     --json repository,path \
     --jq '.[] | "\(.repository.nameWithOwner) \(.path | split("/") | last)"' \
   | sort -u \
   | while read -r repo workflow; do
       if gh workflow run "$workflow" -R "$repo" 2>/dev/null; then
         echo "triggered: $repo / $workflow"
       else
         echo "skipped:   $repo / $workflow (not dispatchable)"
       fi
     done
   ```

   App-auth failure shows up as `Could not authenticate as the GitHub
   App` in the `actions/create-github-app-token` step of the resulting
   run.

# Personal Claude Code Directives

## Version Control
- **Always use Jujutsu (jj)** instead of git for all version control operations
- Use jj commands for commits, branches, and all VCS operations
- **Use Conventional Commits format** for all commit messages:
  - Format: `<type>(<scope>): <subject>` (title max 70 chars)
  - Optional body wrapped at 80 chars per line
  - Types: `feat`, `fix`, `docs`, `chore`, `refactor`, `test`, `style`, `perf`, `ci`, `build`
  - Always include scope when applicable
  - Body should explain the "why" not the "what"

## Output Guidelines
- **NEVER include Claude attribution** in commits, code, documentation, or any other output
- No "Generated with Claude Code" messages
- No "Co-Authored-By: Claude" attributions
- Focus on clean, professional output without AI meta-commentary

## Memory and persistence
- **Don't use the auto-memory system as a substitute for writing things down where I can see them.** Memory files at `~/.claude/projects/.../memory/` are private to you and invisible to me — I can't grep them, version them in dotfiles, or take them with me if I switch tools. From my perspective, anything in memory is not durable.
- **Durable knowledge belongs in files I own and can see:**
  - Cross-project behavioral rules / preferences: this file (`~/.claude/CLAUDE.md`)
  - Project-specific conventions: the project's `CLAUDE.md`
  - Project runbooks, snippets, architecture notes, inventories: a `docs/` folder in the relevant project, or a GitHub issue
- **Memory is only appropriate for** ephemeral within-session/within-project state that genuinely doesn't need to be surfaced to me — and even then, prefer surfacing.
- When you catch yourself reaching for memory to "make something durable," that's the signal to instead propose an edit to one of the files above and let me approve it.

## Shell commands
- **The Bash tool runs on macOS with BSD coreutils, not GNU.** GNU-only flags fail — no `grep -P`/`grep -oP` (Perl regex), no `cat -A`, no GNU-only `sed`/`head`/`date` options. Reach for `rg`, `grep -E`, or `perl` for regex; verify with `man` when unsure. Devshells may ship GNU tools, but the Bash tool doesn't activate `direnv`, so commands run against the base BSD userland.

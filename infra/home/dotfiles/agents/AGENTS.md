# Personal Agent Directives

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
- **NEVER include AI attribution** in commits, code, documentation, or any other output
- No "Generated with" agent messages
- No AI "Co-Authored-By" attributions
- Focus on clean, professional output without AI meta-commentary

## Memory and persistence
- **Don't use an agent's private memory as a substitute for writing things down where I can see them.** Agent-specific memory is not a shared, version-controlled source of truth I can take with me if I switch tools. From my perspective, anything in memory is not durable.
- **Durable knowledge belongs in files I own and can see:**
  - Cross-project behavioral rules / preferences: this file (source: `infra/home/dotfiles/agents/AGENTS.md`, installed at `~/.pi/agent/AGENTS.md` and `~/.claude/CLAUDE.md`)
  - Project-specific conventions: the project's `AGENTS.md`
  - Project runbooks, snippets, architecture notes, inventories: a `docs/` folder in the relevant project, or a GitHub issue
- **Memory is only appropriate for** ephemeral within-session/within-project state that genuinely doesn't need to be surfaced to me — and even then, prefer surfacing.
- When you catch yourself reaching for memory to "make something durable," that's the signal to instead propose an edit to one of the files above and let me approve it.

## Shell commands
- **Check the host and available tools instead of assuming a platform.** macOS commonly has BSD utilities; Linux commonly has GNU utilities. GNU-only flags may fail on macOS. Prefer portable commands and `rg`, `grep -E`, or `perl` for regex; verify support when unsure. Agent shell tools don't necessarily activate `direnv`; explicitly enter the project's devshell when its toolchain is required.

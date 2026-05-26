# Shared home-manager module — applied to the user on both macOS
# (via nix-darwin) and Linux (via NixOS). `home.username` and
# `home.homeDirectory` are set by the host system's user list; this
# module stays platform-agnostic.
{ pkgs, claude-hooks, ... }:

{
  home.stateVersion = "25.05";

  # CLI toolchain. Tools managed via `programs.*` (git, jujutsu, helix,
  # direnv, zsh) install their own binaries; only list the rest here.
  home.packages = [
    pkgs.ripgrep
    pkgs.fd
    pkgs.jq
    pkgs.curl
    pkgs.bat
    pkgs.eza
    pkgs.zellij
    pkgs.claude-code
    # Portable Claude Code harness hooks. The duplicate-issue-create
    # gate (#515) calls `claude-hooks pre-issue-create` from
    # `~/.claude/settings.json` (wired in #534); having it on PATH
    # globally lets the hook fire in any working directory, not just
    # inside the kolohelios checkout.
    claude-hooks.packages.${pkgs.stdenv.hostPlatform.system}.default
  ];

  programs.git = {
    enable = true;
    settings.user = {
      name = "Jon Edwards";
      email = "jkedwards@me.com";
    };
    ignores = [ "**/.claude/settings.local.json" ];
  };

  programs.jujutsu = {
    enable = true;
    settings = {
      user = {
        name = "Jon Edwards";
        email = "jkedwards@me.com";
      };
    };
  };

  programs.helix = {
    enable = true;
    settings = {
      editor = {
        line-number = "absolute";
        auto-format = true;
        soft-wrap.enable = true;
      };
    };
  };

  programs.direnv = {
    enable = true;
    nix-direnv.enable = true;
    # Devenv integration — keeps `use devenv` working in project envrcs
    # that opt into it (none in this repo today, but preserved for
    # parity with the user's existing direnvrc).
    stdlib = ''
      source_url "https://raw.githubusercontent.com/cachix/devenv/d1f7b48e35e6dee421cfd0f51481d17f77586997/direnvrc" "sha256-YBzqskFZxmNb3kYVoKD9ZixoPXJh1C9ZvTLGFRkauZ0="
    '';
  };

  programs.zsh = {
    enable = true;
    shellAliases = {
      ll = "eza -la";
      g = "git";
      j = "jj";
    };
    # Tool-specific PATH exports (Windsurf, nvm, lmstudio, etc.) are
    # not nix-managed; live in ~/.zshrc.local so the generated zshrc
    # doesn't track every IDE installer's path injection.
    initContent = ''
      [[ -f ~/.zshrc.local ]] && source ~/.zshrc.local
    '';
  };

  # Zellij's keybind tree is too elaborate to express idiomatically in
  # nix. Ship the kdl file as-is and let home-manager symlink it.
  xdg.configFile."zellij/config.kdl".source = ../dotfiles/zellij/config.kdl;

  # Claude Code skills that aren't tied to a specific project. Lifted
  # from `kolohelios/.claude/commands/` so they apply to claude
  # sessions in any working directory. Project-specific skills
  # (`/ship`, `/start` — both call `shaka`) stay in kolohelios's
  # `.claude/commands/`.
  home.file.".claude/commands/file-issue.md".source = ../dotfiles/claude/commands/file-issue.md;
}

{
  description = "kolohelios — shared nix utilities for project flakes";

  # Consumed by other project flakes in this repo (tools/shaka,
  # infra/devbox, and the root flake) for a single nixpkgs pin and a
  # shared workflow-tool list. Consumers reference this flake via a
  # `path:` input and `nixpkgs.follows = "kolohelios-nix/nixpkgs"` so
  # the entire repo shares one nixpkgs revision and closures hit cache
  # together. Published to FlakeHub by the build-nix-lib CI job; pushed
  # closures back-fill the substituter for path-input consumers too,
  # since derivations are content-addressed.

  inputs.nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0";

  outputs =
    { self, nixpkgs, ... }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      forEachSupportedSystem =
        f:
        nixpkgs.lib.genAttrs supportedSystems (
          system:
          f {
            inherit system;
            pkgs = import nixpkgs {
              inherit system;
              config.allowUnfree = true;
            };
          }
        );

      # Tiny `shaka` on PATH that walks up from cwd looking for the
      # canonical wrapper at `tools/shaka/bin/shaka` and exec's it.
      # Without this, the documented invocation only works from the repo
      # root — agents in a subdirectory or jj workspace hit `no such file
      # or directory`. Walking up (rather than asking git/jj) handles jj
      # workspaces uniformly, since those have no colocated `.git`.
      shakaShim =
        pkgs:
        pkgs.writeShellApplication {
          name = "shaka";
          text = ''
            dir="$PWD"
            while [[ "$dir" != "/" ]]; do
              wrapper="$dir/tools/shaka/bin/shaka"
              if [[ -x "$wrapper" ]]; then
                exec "$wrapper" "$@"
              fi
              dir="$(dirname "$dir")"
            done
            echo "shaka: no tools/shaka/bin/shaka found above $PWD" >&2
            echo "(this shim expects a kolohelios checkout)" >&2
            exit 1
          '';
        };

      # Workflow tools every project's devShell wants. Consumers compose
      # their own packages by spreading this list and adding project-specific
      # tools on top.
      workflowPackages =
        pkgs:
        (with pkgs; [
          jujutsu
          git
          just
          jq
          cue
          nixfmt-rfc-style
          nil
          typos
          cargo-deny
          cargo-machete
          vale
          _1password-cli
        ])
        ++ [ (shakaShim pkgs) ];
    in
    {
      lib = {
        inherit supportedSystems forEachSupportedSystem workflowPackages;
      };

      formatter = forEachSupportedSystem ({ pkgs, ... }: pkgs.nixfmt-rfc-style);

      # Devshell for working on this project itself (editing the lib's nix
      # files). Same workflow tools every consumer uses.
      devShells = forEachSupportedSystem (
        { pkgs, ... }:
        {
          default = pkgs.mkShell {
            packages = workflowPackages pkgs;
          };
        }
      );
    };
}

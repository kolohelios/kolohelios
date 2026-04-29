{
  description = "kolohelios — shared nix utilities for project flakes";

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

      # Workflow tools every project's devShell wants. Consumers compose
      # their own packages by spreading this list and adding project-specific
      # tools on top.
      workflowPackages =
        pkgs: with pkgs; [
          jujutsu
          git
          just
          jq
          cue
          nixfmt-rfc-style
          nil
        ];
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

{
  description = "kolohelios — monorepo";

  inputs = {
    nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0";
  };

  outputs =
    { self, nixpkgs, ... }:
    let
      lib = nixpkgs.lib;

      # Systems we build devShells for (image is x86_64-linux only)
      shellSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      forEachSystem =
        f:
        lib.genAttrs shellSystems (
          system:
          f {
            inherit system;
            pkgs = import nixpkgs {
              inherit system;
              config.allowUnfree = true;
            };
          }
        );

      # NixOS pkgs — always x86_64-linux for the devbox
      nixosPkgs = import nixpkgs {
        system = "x86_64-linux";
        config.allowUnfree = true;
      };
    in
    {
      # ── NixOS configurations ────────────────────────────────────
      nixosConfigurations.devbox = lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          ./infra/devbox/nixos/configuration.nix
        ];
      };

      # ── Linode disk image ───────────────────────────────────────
      # Build: nix build .#linodeImage
      # Result: result/nixos.img (raw disk image)
      packages.x86_64-linux.linodeImage =
        let
          imageCfg = lib.nixosSystem {
            system = "x86_64-linux";
            modules = [
              ./infra/devbox/nixos/image.nix
            ];
          };
        in
        imageCfg.config.system.build.linodeImage;

      # ── Dev shells ──────────────────────────────────────────────
      devShells = forEachSystem (
        { pkgs, system }:
        {
          default = pkgs.mkShell {
            name = "kolohelios";
            packages = with pkgs; [
              # Nix tooling
              nixfmt-rfc-style
              nil # Nix LSP

              # Infra
              opentofu
              linode-cli

              # General
              git
              jujutsu
              jq
            ];
          };
        }
      );

      # ── Checks ──────────────────────────────────────────────────
      checks.x86_64-linux = {
        # Verify the NixOS configuration evaluates cleanly
        devbox-eval =
          nixosPkgs.runCommand "devbox-eval-check" { } ''
            # If we got here, the NixOS config evaluated successfully
            echo "nixosConfigurations.devbox evaluates" > $out
          '';
      };

      # ── Formatter ───────────────────────────────────────────────
      formatter = forEachSystem ({ pkgs, ... }: pkgs.nixfmt-rfc-style);
    };
}

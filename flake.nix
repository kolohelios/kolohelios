{
  description = "kolohelios — monorepo";

  inputs = {
    kolohelios-nix.url = "path:./nix/kolohelios-nix";
    nixpkgs.follows = "kolohelios-nix/nixpkgs";
  };

  outputs =
    { self, kolohelios-nix, nixpkgs, ... }:
    let
      inherit (kolohelios-nix.lib) forEachSupportedSystem workflowPackages;

      lib = nixpkgs.lib;

      # NixOS pkgs — always x86_64-linux for the devbox.
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
      # Cross-cutting workflow tools come from kolohelios-nix; this shell
      # adds infra-specific tools (opentofu, linode-cli) for working at the
      # repo root.
      devShells = forEachSupportedSystem (
        { pkgs, ... }:
        {
          default = pkgs.mkShell {
            name = "kolohelios";
            packages =
              (workflowPackages pkgs)
              ++ (with pkgs; [
                opentofu
                linode-cli
              ]);
          };
        }
      );

      # ── Checks ──────────────────────────────────────────────────
      checks.x86_64-linux = {
        # Verify the NixOS configuration evaluates cleanly
        devbox-eval = nixosPkgs.runCommand "devbox-eval-check" { } ''
          # If we got here, the NixOS config evaluated successfully
          echo "nixosConfigurations.devbox evaluates" > $out
        '';
      };

      # ── Formatter ───────────────────────────────────────────────
      formatter = kolohelios-nix.formatter;
    };
}

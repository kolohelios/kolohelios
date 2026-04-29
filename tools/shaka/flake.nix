{
  description = "shaka — build tooling for kolohelios";

  inputs = {
    nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { self, ... }@inputs:
    let
      inherit (inputs.nixpkgs) lib;

      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      forEachSupportedSystem =
        f:
        lib.genAttrs supportedSystems (
          system:
          f {
            inherit system;
            pkgs = import inputs.nixpkgs {
              inherit system;
              config.allowUnfree = true;
              overlays = [ inputs.rust-overlay.overlays.default ];
            };
          }
        );

      rustToolchain =
        pkgs:
        pkgs.rust-bin.nightly.latest.default.override {
          extensions = [
            "rust-src"
            "rust-analyzer"
            "llvm-tools-preview"
          ];
        };
    in
    {
      packages = forEachSupportedSystem ({ pkgs, ... }: {
        default = pkgs.rustPlatform.buildRustPackage {
          pname = "shaka";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          # Integration tests in tests/schema.rs shell out to `cue vet`
          # against the shipped schema fixtures. Make cue available during
          # the check phase so `nix flake check` (and CI's `shaka preflight`)
          # can run them in the build sandbox.
          nativeCheckInputs = [ pkgs.cue ];
        };
      });

      apps = forEachSupportedSystem ({ system, ... }: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/shaka";
        };
      });

      devShells = forEachSupportedSystem (
        { pkgs, system }:
        {
          default = pkgs.mkShell {
            packages = [
              (rustToolchain pkgs)
              pkgs.jq
              self.formatter.${system}
            ];
          };
        }
      );

      formatter = forEachSupportedSystem ({ pkgs, ... }: pkgs.nixfmt);
    };
}

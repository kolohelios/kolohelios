{
  description = "kolohelios — portfolio static origin (Cloudflare Pages)";

  inputs = {
    kolohelios-nix.url = "https://flakehub.com/f/kolohelios/kolohelios-nix/*.tar.gz";
    nixpkgs.follows = "kolohelios-nix/nixpkgs";
  };

  outputs =
    {
      self,
      kolohelios-nix,
      nixpkgs,
      ...
    }:
    let
      inherit (kolohelios-nix.lib) forEachSupportedSystem workflowPackages;
    in
    {
      devShells = forEachSupportedSystem (
        { pkgs, ... }:
        {
          default = pkgs.mkShell {
            packages =
              (workflowPackages pkgs)
              ++ (with pkgs; [
                opentofu
                _1password-cli
                wrangler
              ]);
          };
        }
      );

      formatter = kolohelios-nix.formatter;
    };
}

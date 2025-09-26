
{
  description = "dbdrill";

  inputs = {
    nixpkgs.url  = "github:NixOS/nixpkgs/nixos-25.05";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
      };
    };
    flake-utils.url  = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, crane, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        craneLib = (crane.mkLib pkgs).overrideToolchain rust;

        craneCommonArgs = {
          src = craneLib.cleanCargoSource ./.;
          strictDeps = true;
          buildInputs = [] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [];
        };

        dbdrill = craneLib.buildPackage(
          craneCommonArgs // { cargoArtifacts = craneLib.buildDepsOnly craneCommonArgs; }
        );
      in
      with pkgs;
      {
        checks = { inherit dbdrill; };
        packages.default = dbdrill;
        apps.default = flake-utils.lib.mkApp { drv = dbdrill; };
        devShells.default = mkShell {
          buildInputs = [
            rust
          ];
        };
      }
    );
}


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

        src = craneLib.cleanCargoSource ./.;

        craneCommonArgs = {
          inherit src;
          strictDeps = true;
          buildInputs = [] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [];
        };

        cargoArtifacts = craneLib.buildDepsOnly craneCommonArgs;

        dbdrill = craneLib.buildPackage(
          craneCommonArgs // { inherit cargoArtifacts; }
        );
      in
      with pkgs;
      {
        checks = {
          # Make sure it compiles
          inherit dbdrill;

          dbdrill-clippy = craneLib.cargoClippy ( craneCommonArgs // { inherit cargoArtifacts; } );
          dbdrill-fmt = craneLib.cargoFmt { inherit src; };
        };
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


{
  description = "dbdrill";

  inputs = {
    nixpkgs.url  = "github:NixOS/nixpkgs/nixos-26.05";
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

        version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;

        craneCommonArgs = {
          inherit src version;
          pname = "dbdrill";
          strictDeps = true;
          nativeBuildInputs = pkgs.lib.optionals pkgs.stdenv.isLinux [
            pkgs.pkg-config
          ];
          # native-tls uses OpenSSL on Linux, and the Security framework on
          # Darwin.
          buildInputs = pkgs.lib.optionals pkgs.stdenv.isLinux [
            pkgs.openssl
          ];
        };

        cargoArtifacts = craneLib.buildDepsOnly craneCommonArgs;

        dbdrill = craneLib.buildPackage(
          craneCommonArgs // {
            inherit cargoArtifacts;
            cargoExtraArgs = "--locked --package dbdrill-tui";
          }
        );

        # winit and glutin dlopen the windowing and GL libraries, so they never
        # show up in the binary as ELF dependencies. Put them in the RPATH by
        # hand, otherwise the GUI only starts where the host happens to provide
        # them.
        guiRuntimeLibs = with pkgs; [
          libGL
          libxkbcommon
          wayland
          xorg.libX11
          xorg.libXcursor
          xorg.libXi
          xorg.libXinerama
          xorg.libXrender
          xorg.libxcb
        ];

        dbdrillui = craneLib.buildPackage(
          craneCommonArgs // {
            inherit cargoArtifacts;
            pname = "dbdrillui";
            cargoExtraArgs = "--locked --package dbdrill-egui";
            postFixup = pkgs.lib.optionalString pkgs.stdenv.isLinux ''
              patchelf --add-rpath ${pkgs.lib.makeLibraryPath guiRuntimeLibs} \
                $out/bin/dbdrillui
            '';
          }
        );
      in
      with pkgs;
      {
        checks = {
          # Make sure it compiles
          inherit dbdrill;

          dbdrill-clippy = craneLib.cargoClippy ( craneCommonArgs // { inherit cargoArtifacts; } );
          dbdrill-fmt = craneLib.cargoFmt { inherit src; };
          dbdrill-test = craneLib.cargoTest ( craneCommonArgs // { inherit cargoArtifacts; } );
        };
        packages = {
          default = dbdrill;
          inherit dbdrill dbdrillui;
        };
        apps.default = flake-utils.lib.mkApp { drv = dbdrill; };
        devShells.default = mkShell {
          buildInputs = [
            rust
          ];
        };
      }
    );
}

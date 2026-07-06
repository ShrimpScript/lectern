{
  description = "Lectern — a local-first, Linux-native engine for orchestrating AI coding agents with a shared brain";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };

        lectern = pkgs.rustPlatform.buildRustPackage {
          pname = "lectern";
          version = "0.5.0";

          src = pkgs.lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;

          # The workspace ships two binaries: `lectern` (CLI) and `lecternd`
          # (the scheduler daemon). The desktop app is Tauri/GUI and the terminal
          # UI is a separate Bun package, so neither is part of the Nix install.
          #
          # No system libraries are required: `rusqlite` uses its `bundled`
          # feature (SQLite is compiled from source), and HTTP goes through
          # `ureq` on rustls, so there is no OpenSSL dependency.
          nativeBuildInputs = [ pkgs.pkg-config ];

          # Unit tests bind local TCP sockets and are skipped in the sandbox.
          doCheck = false;

          meta = with pkgs.lib; {
            description = "Local-first engine for orchestrating multiple AI coding agents with a shared brain";
            homepage = "https://github.com/ShrimpScript/lectern";
            license = licenses.asl20;
            mainProgram = "lectern";
            platforms = platforms.linux ++ platforms.darwin;
          };
        };
      in
      {
        packages.default = lectern;
        packages.lectern = lectern;

        apps.default = flake-utils.lib.mkApp {
          drv = lectern;
          name = "lectern";
        };

        devShells.default = pkgs.mkShell {
          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.rustfmt
            pkgs.clippy
            pkgs.pkg-config
          ];
        };
      }
    );
}

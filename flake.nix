{
  description = "rlm — The Context Broker: semantic code exploration for AI agents";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    let
      # System-independent outputs.
      overlays.default = final: _prev: {
        rlm = self.packages.${final.system}.default;
      };
    in
    {
      inherit overlays;
    }
    // flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        lib = pkgs.lib;
        cargoToml = lib.importTOML ./Cargo.toml;

        # Drop build artefacts, scratch dirs, and `.git/` from the
        # source tree so the store hash stays stable across local cargo
        # runs. `lib.fileset.gitTracked` would be more idiomatic but it
        # only works when the flake is fetched via git (`github:`); it
        # errors out under `path:.` (NixOS/nix#9292), which is the
        # workflow the CONTRIBUTING smoke test uses.
        src = lib.cleanSourceWith {
          src = ./.;
          filter = path: _type:
            let base = baseNameOf (toString path);
            in !(builtins.elem base [
              ".git"
              "target"
              "result"
              ".rlm"
              ".direnv"
              ".cargo"
              "work"
              "docs"
            ]);
        };

        rlm = pkgs.rustPlatform.buildRustPackage {
          pname = cargoToml.package.name;
          version = cargoToml.package.version;
          inherit src;

          cargoLock = {
            lockFile = ./Cargo.lock;
            # No outputHashes block: every dependency in Cargo.lock comes
            # from crates.io. If a git dep is ever added, populate this
            # map with the SRI hash Nix prints in the error message.
          };

          # rusqlite uses the `bundled` feature and every tree-sitter
          # grammar vendors its C parser — stdenv's C toolchain is all
          # that's needed. Add `pkg-config` here only when a future dep
          # actually probes for a system library.
          nativeBuildInputs = [ ];
          buildInputs = [ ];

          # Skip `cargo test` inside the Nix sandbox: the e2e suite shells
          # out to the freshly built `rlm` binary against on-disk fixtures,
          # which is brittle here. The full suite still runs via
          # `cargo nextest run` locally and in CI.
          doCheck = false;

          meta = {
            description = cargoToml.package.description;
            homepage = cargoToml.package.repository;
            license = lib.licenses.mit;
            mainProgram = "rlm";
            platforms = lib.platforms.unix;
          };
        };
      in
      {
        packages = {
          default = rlm;
          rlm = rlm;
        };

        apps.default = {
          type = "app";
          program = lib.getExe rlm;
        };

        devShells.default = pkgs.mkShell {
          # `inputsFrom = [ rlm ]` pulls rlm's build inputs (rustc, cargo
          # via rustPlatform's hook) into the shell. `packages` adds only
          # the dev-only extras on top.
          inputsFrom = [ rlm ];
          packages = with pkgs; [
            rustfmt
            clippy
            cargo-nextest
            rust-analyzer
          ];
        };
      });
}

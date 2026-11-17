{
  description = "Menagerie: compile Lox, Monkey and Tiger to one common bytecode, and interpret it";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/release-25.05";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
    let
      pkgs = nixpkgs.legacyPackages.${system};
      lib = pkgs.lib;

      menagerie = pkgs.rustPlatform.buildRustPackage {
        pname = "menagerie";
        version = "0.1.0";
        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./src ];
        };
        cargoLock.lockFile = ./Cargo.lock;
        meta = {
          description = "Compiler from Lox, Monkey and Tiger to a common bytecode, and its interpreter";
          license = lib.licenses.mit;
          mainProgram = "menageriec";
        };
      };

      # The conformance and golden tests, run against the built binaries.
      tests = pkgs.runCommand "menagerie-tests" {
        nativeBuildInputs = [ pkgs.python3 ];
      } ''
        python3 ${./tests/run_lox_suite.py} ${menagerie}/bin/menageriec ${./tests/lox-suite}
        python3 ${./tests/run_cases.py} ${menagerie}/bin ${./tests/cases} ${./examples}
        python3 ${./tests/fuzz_bytecode.py} ${menagerie}/bin ${./tests/cases} 1500
        touch $out
      '';
    in {
      packages = {
        inherit menagerie;
        default = menagerie;
      };

      apps = {
        menageriec = flake-utils.lib.mkApp { drv = menagerie; name = "menageriec"; };
        menagerie-vm = flake-utils.lib.mkApp { drv = menagerie; name = "menagerie-vm"; };
        default = self.apps.${system}.menageriec;
      };

      checks = {
        inherit menagerie tests;
      };

      devShells.default = pkgs.mkShell {
        packages = [ pkgs.cargo pkgs.rustc pkgs.clippy pkgs.rustfmt pkgs.python3 ];
      };
    });
}

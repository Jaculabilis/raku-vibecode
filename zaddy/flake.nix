{
  description = "A collection of metacompiling programs";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/release-25.05";
    flake-utils.url = "github:numtide/flake-utils";
    rpypkgs = {
      url = "github:rpypkgs/rpypkgs";
      inputs = {
        nixpkgs.follows = "nixpkgs";
        flake-utils.follows = "flake-utils";
      };
    };
  };

  outputs = { self, nixpkgs, flake-utils, rpypkgs }:
    flake-utils.lib.eachDefaultSystem (system:
    let
      pkgs = nixpkgs.legacyPackages.${system};
      bootstrap = rpypkgs.lib.${system}.mkRPythonDerivation {
        entrypoint = "zaddy.py";
        binName = "zaddyc";
        optLevel = "2";
      } {
        pname = "zaddy";
        version = "2025.4";
        src = ./rzaddy;
      };
      mk = desc: binName: let
        src = pkgs.stdenv.mkDerivation {
          name = "${binName}-src";
          src = ./.;
          installPhase = ''
            mkdir $out/
            ${bootstrap}/bin/zaddyc < ${desc} > $out/src.py
          '';
        };
      in rpypkgs.lib.${system}.mkRPythonDerivation {
        entrypoint = "src.py";
        inherit binName;
        optLevel = "2";
      } {
        pname = "zaddy";
        version = "2025.4";
        inherit src;
      };
    in {
      packages = rec {
        default = zaddyc;
        zaddyc = mk "zaddy.zaddy" "zaddyc";
        bfc = mk "bf.zaddy" "bfc";
        arithc = mk "arith.zaddy" "arithc";
      };
      checks = let
        zaddyc = self.packages.${system}.zaddyc;
        bfc = self.packages.${system}.bfc;
        arithc = self.packages.${system}.arithc;
      in {
        # zaddy.zaddy compiled by itself must reproduce the bootstrap text.
        fixedpoint = pkgs.runCommand "zaddy-fixedpoint" {} ''
          ${zaddyc}/bin/zaddyc < ${./zaddy.zaddy} > out.py
          cmp out.py ${./rzaddy/zaddy.py}
          touch $out
        '';
        # Brainfuck to C, with peephole optimizations, compiled and run.
        bf = pkgs.runCommand "zaddy-bf" { nativeBuildInputs = [ pkgs.gcc ]; } ''
          ${bfc}/bin/bfc < ${./tests/hello.b} > hello.c
          grep -q '\*ptr = 3;' hello.c
          grep -q 'ptr + 2) += \*ptr \* 1' hello.c
          gcc -o hello hello.c
          ./hello > hello.txt
          cmp hello.txt ${./tests/hello.out}
          touch $out
        '';
        # Simplification modulo AC, one expression per run.
        arith = pkgs.runCommand "zaddy-arith" {} ''
          while IFS= read -r line; do
            echo "$line" | ${arithc}/bin/arithc
          done < ${./tests/arith.in} > got.txt
          diff -u ${./tests/arith.out} got.txt
          touch $out
        '';
      };
      devShells.default = pkgs.mkShell {
        packages = with pkgs; [
          gdb
        ];
      };
    });
}

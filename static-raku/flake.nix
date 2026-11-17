{
  description = "Fully static Raku (Rakudo on MoarVM) for Linux, linked against musl";

  # Same nixpkgs revision the research was done against (Rakudo 2025.06.1).
  inputs.nixpkgs.url = "github:nixos/nixpkgs/cff6bcc05680cea7406aa1f293d76be9c1483f02";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems =
        f: nixpkgs.lib.genAttrs systems (system: f system nixpkgs.legacyPackages.${system}.pkgsMusl);
    in
    {
      packages = forAllSystems (
        system: pkgs:
        let
          static = import ./default.nix { inherit pkgs; };
        in
        static // { default = static.raku; }
      );

      checks = forAllSystems (
        system: pkgs:
        import ./checks.nix {
          inherit pkgs;
          hostPkgs = nixpkgs.legacyPackages.${system};
          inherit (self.packages.${system})
            raku
            rakudo
            nqp
            moarvm
            ;
        }
      );
    };
}

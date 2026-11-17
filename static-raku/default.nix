# Fully static Rakudo for Linux: MoarVM -> NQP -> Rakudo, linked against musl.
#
# `pkgs` must be a musl package set (pkgsMusl). We reuse the nixpkgs
# definitions for sources, versions and metadata and change only how they
# are linked.
#
# Stock Rakudo cannot be static because every compiler module runs
#   loadlib dynext/libperl6_ops_moar.so; loadext Rakudo_ops_init
# when loaded, and dlopen always fails in a static musl executable. We compile
# those extension ops into libmoar instead (patches/moarvm-builtin-rakudo-ops)
# and stop Rakudo from building the shared library (patches/rakudo-no-dynext).
{ pkgs }:
let
  upstream = { inherit (pkgs) moarvm nqp rakudo; };
in
rec {
  moarvm = upstream.moarvm.overrideAttrs (old: {
    pname = "moarvm-static";
    patches = (old.patches or [ ]) ++ [ ./patches/moarvm-builtin-rakudo-ops.patch ];
    # Perl only runs Configure.pl; it is not a runtime dependency.
    buildInputs = [ ];
    nativeBuildInputs = [ pkgs.perl ];
    preConfigure = ''
      configureFlagsArray+=(
        --static
        "--ld=cc -static"
        --builtin-rakudo-ops=${upstream.rakudo.src}/src/vm/moar/ops/perl6_ops.c
      )
    '';
    # A --static install ships only libmoar.a, not the bundled 3rdparty
    # archives it depends on (libuv, libtommath, cmp, dyncall, sha1), so the
    # NQP and Rakudo launchers would fail to link. Fold them into libmoar.a.
    postInstall = ''
      {
        echo "create libmoar-full.a"
        echo "addlib $out/lib/libmoar.a"
        find 3rdparty -name '*.a' -printf 'addlib %p\n'
        echo save
        echo end
      } | ar -M
      mv libmoar-full.a $out/lib/libmoar.a
    '';
  });

  nqp = (upstream.nqp.override { inherit moarvm; }).overrideAttrs (old: {
    pname = "nqp-static";
    buildInputs = [ ];
    nativeBuildInputs = [ pkgs.perl ];
    # NativeCall needs dlopen, which a static executable doesn't have:
    # buildnativecall fails with "Dynamic loading not supported", and this
    # test then calls the unbuilt function anyway and segfaults.
    preCheck = ''
      rm t/nativecall/01-basic.t
    '';
  });

  rakudo = (upstream.rakudo.override { inherit nqp; }).overrideAttrs (old: {
    pname = "rakudo-static";
    patches = (old.patches or [ ]) ++ [ ./patches/rakudo-no-dynext.patch ];
    # icu, zlib and gmp are not used by Rakudo on MoarVM; perl only runs
    # Configure.pl.
    buildInputs = [ ];
    nativeBuildInputs = [ pkgs.perl ];
    # Upstream scrubs a compiler reference out of the dynext library, which
    # no longer exists.
    postFixup = "";
    doInstallCheck = true;
    installCheckPhase = ''
      runHook preInstallCheck
      $out/bin/raku -e 'say "hello from static raku"'
      runHook postInstallCheck
    '';
  });

  # Just what is needed to run Raku, for initramfs: one static `raku` plus
  # the NQP and Rakudo bytecode, with no references to other store paths.
  # The `rakudo` closure, by contrast, drags in Perl, bash, sources and
  # libmoar.a via configure strings, wrapper scripts and NQP's MAST symlinks.
  #
  # The C runner finds its bytecode through paths compiled in with -D (bare;
  # main.c stringifies them), so we recompile that one file against this
  # output's own layout.
  raku = pkgs.stdenv.mkDerivation {
    pname = "raku-static";
    inherit (rakudo) version;
    src = rakudo.src;
    nativeBuildInputs = [ pkgs.removeReferencesTo ];
    dontConfigure = true;

    buildPhase = ''
      runHook preBuild
      moarConfig() { ${nqp}/bin/nqp --show-config | sed -n "s/^moar::$1=//p"; }
      cc -static $(moarConfig cflags) \
        -DSTATIC_EXEC_PATH=$out/bin/raku \
        -DSTATIC_NQP_HOME=$out/share/nqp \
        -DSTATIC_RAKUDO_HOME=$out/share/perl6 \
        ${
          pkgs.lib.concatMapStringsSep " " (d: "-I${moarvm}/include/${d}") [
            "moar"
            "libuv"
            "libtommath"
            "dyncall"
            "mimalloc"
          ]
        } \
        -o raku src/vm/moar/runner/main.c ${moarvm}/lib/libmoar.a $(moarConfig ldlibs)
      runHook postBuild
    '';

    installPhase = ''
      runHook preInstall
      install -Dm755 raku $out/bin/raku
      mkdir -p $out/share/nqp/lib $out/share/perl6/runtime
      cp ${nqp}/share/nqp/lib/*.moarvm $out/share/nqp/lib/
      cp -r ${rakudo}/share/perl6/{lib,core,site,vendor} $out/share/perl6/
      cp ${rakudo}/share/perl6/runtime/*.moarvm $out/share/perl6/runtime/
      rm $out/share/perl6/runtime/*debug*
      chmod -R u+w $out/share
      runHook postInstall
    '';

    # What still names other store paths is informational: build
    # configuration strings ($*VM.config, $*RAKU.compiler), source file names
    # in backtrace annotations, and the repo config.json files. Nothing opens
    # them (the initramfs-boot check runs with only this closure), so scrub
    # them; remove-references-to keeps string lengths, so bytecode stays valid.
    postFixup = ''
      find $out -type f -exec remove-references-to ${
        pkgs.lib.concatMapStringsSep " " (p: "-t ${p}") [
          moarvm
          nqp
          rakudo
          rakudo.src
          pkgs.perl
        ]
      } {} +
    '';
    disallowedReferences = [
      moarvm
      nqp
      rakudo
      pkgs.perl
      pkgs.bash
    ];

    doInstallCheck = true;
    installCheckPhase = ''
      $out/bin/raku -e 'say "hello from slim static raku"'
    '';
  };
}

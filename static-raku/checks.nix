{
  pkgs,
  # glibc package set, for the kernel, QEMU and initrd tooling
  hostPkgs,
  raku,
  rakudo,
  nqp,
  moarvm,
}:
{
  # Every executable is a static ELF with no program interpreter, and no
  # shared libraries were installed for anything to dlopen.
  static-binaries =
    pkgs.runCommand "static-raku-is-static"
      {
        nativeBuildInputs = [
          pkgs.file
          pkgs.binutils
        ];
      }
      ''
        for exe in ${moarvm}/bin/moar ${nqp}/bin/nqp-m ${rakudo}/bin/rakudo ${raku}/bin/raku; do
          echo "$exe: $(file -bL "$exe")"
          file -bL "$exe" | grep -q 'statically linked'
          if readelf -lW "$(readlink -f "$exe")" | grep -q INTERP; then
            echo "$exe has a program interpreter" >&2
            exit 1
          fi
        done
        if find -L ${moarvm} ${nqp} ${rakudo} ${raku} \( -name '*.so' -o -name '*.so.[0-9]*' \) | grep .; then
          echo "shared libraries were installed" >&2
          exit 1
        fi
        touch $out
      '';

  # The cut-down Vixen compiler runs on the slim static raku with an empty
  # environment, including the subprocess calls back into "Vixen".
  vixen-mini = pkgs.runCommand "static-raku-vixen-mini" { } ''
    export HOME=$TMPDIR
    env -i HOME=$HOME TMPDIR=$TMPDIR \
      ${raku}/bin/raku ${./tests/vixen-mini.raku} > actual
    diff -u ${./tests/vixen-mini.expected} actual
    touch $out
  '';

  # Boot a kernel whose initramfs holds nothing but the slim static raku
  # (which references no other store path) and a static busybox (for the /bin/sh scripts "Vixen" runs), with a Raku
  # script as /init. There is no libc, no dynamic loader and no /proc.
  initramfs-boot = import ./tests/initramfs-boot.nix {
    inherit hostPkgs raku;
    vixenTest = ./tests/vixen-mini.raku;
    vixenExpected = ./tests/vixen-mini.expected;
  };
}

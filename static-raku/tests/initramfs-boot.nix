# Boot Linux in QEMU with an initramfs containing only the slim static raku,
# a static busybox as /bin/sh, and a Raku script as /init.
{
  hostPkgs,
  raku,
  vixenTest,
  vixenExpected,
}:
let
  inherit (hostPkgs) lib stdenv;
  arch = stdenv.hostPlatform;
  kernel = hostPkgs.linuxPackages.kernel;
  busybox = hostPkgs.pkgsStatic.busybox;

  qemu =
    if arch.isx86_64 then
      "${hostPkgs.qemu_test}/bin/qemu-system-x86_64 -machine q35"
    else if arch.isAarch64 then
      "${hostPkgs.qemu_test}/bin/qemu-system-aarch64 -machine virt -cpu max"
    else
      throw "initramfs-boot: unsupported platform ${arch.system}";
  console = if arch.isx86_64 then "ttyS0" else "ttyAMA0";

  init = hostPkgs.writeTextFile {
    name = "init.raku";
    executable = true;
    text = ''
      #!${raku}/bin/raku
      # PID 1 in a bare initramfs.
      say "STATIC-RAKU: Rakudo { $*RAKU.compiler.version }, PID $*PID";
      say "STATIC-RAKU: /proc mounted? { '/proc/self'.IO.e }";
      mkdir '/tmp';
      %*ENV<TMPDIR> = '/tmp';
      my $proc = run '${raku}/bin/raku', '${vixenTest}', :out, :err;
      my $actual = $proc.out.slurp: :close;
      note $proc.err.slurp: :close;
      say $actual eq slurp('${vixenExpected}')
          ?? 'STATIC-RAKU: vixen-mini PASS'
          !! "STATIC-RAKU: vixen-mini FAIL\n$actual";
    '';
  };

  initrd = hostPkgs.makeInitrd {
    contents = [
      {
        object = init;
        symlink = "/init";
      }
      {
        object = "${busybox}/bin/busybox";
        symlink = "/bin/sh";
      }
    ];
  };
in
hostPkgs.runCommand "static-raku-initramfs-boot"
  {
    requiredSystemFeatures = [ "kvm" ];
  }
  ''
    # When /init exits the kernel panics; panic=-1 + -no-reboot ends QEMU.
    timeout 900 ${qemu} -accel kvm -accel tcg -m 1024 -smp 2 \
      -kernel ${kernel}/${arch.linux-kernel.target} \
      -initrd ${initrd}/initrd \
      -append "console=${console} panic=-1 loglevel=3" \
      -display none -monitor none -serial file:console.log -no-reboot || true
    grep STATIC-RAKU console.log || { tail -40 console.log; exit 1; }
    grep -q 'STATIC-RAKU: vixen-mini PASS' console.log
    cp console.log $out
  ''

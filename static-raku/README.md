# Static Raku

Fully static Raku (Rakudo 2025.06.1 on MoarVM 2025.06) for Linux, linked
against musl, for use in an initramfs. Builds on `x86_64-linux` and
`aarch64-linux`.

```
nix build            # slim bundle: bin/raku + bytecode, 70 MiB, no references
nix build .#rakudo   # full static Rakudo install (also .#nqp, .#moarvm)
nix flake check      # static-ness, Vixen smoke test, initramfs boot in QEMU
```

## Why stock Rakudo can't be static

MoarVM and NQP link statically without trouble. Rakudo's build passes
`--vmlibs=dynext/libperl6_ops_moar.so=Rakudo_ops_init` to NQP, so every
Rakudo compiler module runs `loadlib` + `loadext` when it loads. `loadlib`
calls `dlopen`, and in a static musl executable `dlopen` always fails
("Dynamic loading not supported"). See `../RESEARCH.md` for the trace.

## The fix

- `patches/moarvm-builtin-rakudo-ops.patch` adds
  `Configure.pl --builtin-rakudo-ops=<perl6_ops.c>`. This compiles Rakudo's
  ten extension ops into libmoar, and adds a table of built-in extensions to
  `src/core/dll.c`: a `loadlib` naming `libperl6_ops_moar` is satisfied
  without `dlopen`, and `Rakudo_ops_init` is looked up in the table rather
  than with `dlsym`. Without the flag, MoarVM behaves exactly as before.
- `patches/rakudo-no-dynext.patch` stops Rakudo building and installing
  the `.so`. The `--vmlibs` path remains only as the name MoarVM matches.
- `default.nix` builds MoarVM with `--static`, folds its bundled 3rdparty
  archives into `libmoar.a` (a `--static` install omits them), and builds NQP
  and Rakudo on top. The `raku` package recompiles the one-file C runner so
  its compiled-in home paths point into a self-contained bundle.

## Limits

- NativeCall needs `dlopen`, so it does not work. `buildnativecall` throws
  "Dynamic loading not supported". NQP's `t/nativecall/01-basic.t` is
  skipped because it calls the unbuilt function anyway and segfaults. That's
  a pre-existing MoarVM bug, not something this patch introduced.
- `raku` is one static executable plus about 62 MiB of bytecode in
  `share/`, not a single file. MoarVM can load bytecode from memory
  (`MVM_cu_from_bytes`), so embedding it in the ELF is possible future work.

## Checks

- `static-binaries`: `moar`, `nqp-m`, `rakudo` and `raku` are static ELF
  files with no `PT_INTERP`, and no `.so` files are installed.
- `vixen-mini`: `tests/vixen-mini.raku` (the Vixen grammar verbatim from the
  blog post, plus actions, `Set` name analysis, execline emission and the
  `callVixen` subprocess FFI) gives exactly `tests/vixen-mini.expected`.
- `initramfs-boot`: boots the nixpkgs kernel in QEMU with an initramfs
  containing only the `raku` bundle and a static busybox (`/bin/sh` for the
  Vixen callbacks), with a Raku script as `/init`. Runs with no `/proc`.

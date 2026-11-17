# Task 2 research: why can't Raku be statically compiled?

Versions examined: MoarVM 2025.06, NQP 2025.06.1, Rakudo 2025.06.1 (the
versions in nixpkgs `cff6bcc0`), musl 1.2.5, target `x86_64-linux`.

## Short answer

The VM is not the problem. **MoarVM and NQP both build and run as fully
static musl executables.** Rakudo breaks because its compiler bytecode
calls `dlopen` on a small C plugin, `libperl6_ops_moar.so`, at load time.
`dlopen` cannot work in a static musl binary. This is a design choice in the
Rakudo build, not a fundamental limitation, and it looks patchable.

## Evidence (reproducible: `nix/static-rakudo.nix`, logs in `logs/`)

| Step | Result |
|------|--------|
| `nixpkgs#pkgsStatic.rakudo` | Does not even evaluate: `postFixup` references `hostPlatform.extensions.sharedLibrary`, which doesn't exist on static platforms (`logs/pkgsStatic-x86_64.log`). |
| MoarVM `Configure.pl --static --ld="cc -static"` (pkgsMusl) | OK. `bin/moar: ELF 64-bit LSB executable, x86-64, statically linked` (`logs/exp-moarvm.log`). |
| NQP on static MoarVM | Full NQP bootstrap runs on the static `moar`. Linking the `nqp` C launcher then fails with 1852 undefined symbols (`uv_*`, `mp_*`, `cmp_*`, `dc*`): a `--static` MoarVM install ships only `libmoar.a`, not its bundled 3rdparty archives (`logs/exp-nqp.log`). **Workaround #1:** merge `libuv.a libtommath.a libdyncall_s.a libdynload_s.a libdyncallback_s.a libcmp.a libsha1.a` into `libmoar.a`. After that, `nqp` is statically linked and prints `hello from static nqp` (`logs/exp-nqp2.log`). |
| Rakudo on static NQP | Fails linking `dynext/libperl6_ops_moar.so`. It uses MoarVM's linker (`cc -static`) plus `-shared` and dies with `relocation R_X86_64_32 against hidden symbol '__TMC_END__' can not be used when making a shared object` (`logs/exp-rakudo.log`). |
| Same, with the `.so` linked as a normal shared object (**workaround #2**, diagnostic only) | The build compiles all of `Perl6::*` and `rakudo.moarvm`. It dies the first time the static VM *executes* Rakudo compiler bytecode (building `BOOTSTRAP/v6c.moarvm`): **`failed to load library 'dynext/libperl6_ops_moar.so'`** (`logs/exp-rakudo2.log`). |

## Root cause, traced through the source

1. **Rakudo's build asks NQP to bake a dlopen into every compiler module.**
   `rakudo/tools/templates/moar/Makefile.in:99`:
   `NQP_FLAGS_EXTRA = --vmlibs=dynext/libperl6_ops_moar.so=Rakudo_ops_init`
2. **NQP turns `--vmlibs` into `loadlib` + `loadext` ops** in each compilation
   unit's deserialization/fixup code, so they run whenever the module loads
   (`nqp/src/NQP/World.nqp:490-516`).
3. **`loadlib` → `MVM_dll_load` → `dlopen`.** `moarvm/src/core/dll.c:26-31`
   calls `MVM_nativecall_load_lib`, which is `dlLoadLibrary` (dyncall) or
   `dlopen(path, RTLD_NOW|RTLD_GLOBAL)` (libffi), per
   `moarvm/src/core/nativecall_{dyncall,libffi}.h`. Failure throws a fatal
   `failed to load library`.
4. **`loadext` → `MVM_ext_load`** (`moarvm/src/core/ext.c:3-50`) finds the
   symbol `Rakudo_ops_init` in that library and calls it.
5. **In static musl, `dlopen` always fails, by design.**
   `musl/src/ldso/dlopen.c`: `stub_dlopen` → `"Dynamic loading not
   supported"`, `return 0`. (Static glibc has only a fragile partial `dlopen`.
   It needs matching shared glibc at runtime, and the plugin needs `libmoar`
   symbols exported from the executable, which defeats the purpose.)
6. **What the plugin contains** (`rakudo/src/vm/moar/ops/perl6_ops.c`, 190
   lines): ten ext-ops registered with `MVM_ext_register_extop`:
   `p6init p6captureouters p6stateinit p6setfirstflag p6takefirstflag p6setpre
   p6clearpre p6inpre p6staticouter p6invokeunder`. Nothing in it actually
   needs to be dynamically loaded.

### Secondary obstacles (not blockers, but relevant to initramfs)

- **Raku is not one file.** The `raku` launcher
  (`rakudo/src/vm/moar/runner/main.c`) finds `NQP_HOME`/`RAKUDO_HOME`
  relative to its own path via `uv_exepath` (i.e. `/proc/self/exe`; `/proc`
  may not be mounted early in initramfs) unless the homes are compiled in
  (non-relocatable build). It then `mmap`s `.moarvm` bytecode from disk
  (`moarvm/src/core/compunit.c:40-60`). A stock install has 22 `.moarvm`
  files. `CORE.c.setting.moarvm` alone is 30 MB, and `share/perl6/runtime`
  plus `share/perl6/lib` together are about 52 MB.
- MoarVM can already load bytecode from memory (`loadbytecodebuffer` op,
  `MVM_cu_from_bytes`), so embedding the bytecode in the ELF is plausible but
  not implemented. TPF's outreach idea "Linking Perl 6 Compiler and Programs"
  proposed exactly this and was never completed.
- nixpkgs packaging bugs: `moarvm.nix` lists `perl` as a `buildInput`
  (pkgsStatic builds a whole static Perl for nothing) and never passes
  `--static`. `rakudo.nix` assumes a shared-library extension.
- The Vixen compiler itself needs only core Raku: grammars, actions,
  classes/roles, `Set`, `run`, `slurp`/`spurt`/`chmod`, `%*ENV`. It uses no
  NativeCall and no modules, so the remaining NativeCall `dlopen` use isn't
  needed.

## Result: S tier, Raku can be statically compiled and linked

Implemented in `static-raku/` (a flake; see its README):

1. `patches/moarvm-builtin-rakudo-ops.patch` adds MoarVM
   `Configure.pl --builtin-rakudo-ops=<perl6_ops.c>`. It compiles the ops into
   libmoar and adds a built-in extension table in `src/core/dll.c`, so
   `loadlib …/libperl6_ops_moar.*` is satisfied without `dlopen` and
   `Rakudo_ops_init` comes from the table instead of `dlsym`. With the flag
   off, behaviour is unchanged. The ops go in libmoar, not the `raku`
   runner, because the static `nqp` also runs Rakudo compiler code during
   the build.
2. `patches/rakudo-no-dynext.patch` stops building and installing the `.so`.
3. `default.nix` (MoarVM `--static`, 3rdparty archives folded into
   `libmoar.a`, NQP, Rakudo) plus a slim `raku` bundle. The bundle
   recompiles the one-file runner against its own layout: one static `raku`
   and 62 MiB of bytecode, 69.5 MiB total, with no references to other store
   paths. The full `rakudo` closure was 257 MiB and dragged in Perl, bash and
   sources.

Verified on `x86_64-linux` and `aarch64-linux` (`logs/checks-all.log`):
- NQP's own test suite passes: 143 files, 13469 tests. `t/nativecall/01-basic.t`
  is skipped because NativeCall needs `dlopen`.
- All of `moar`, `nqp-m`, `rakudo` and `raku` are static, with no `PT_INTERP`.
- A cut-down Vixen compiler (the blog grammar verbatim, plus actions, `Set`
  name analysis, execline output, and `callVixen` via `run`) produces the
  expected output on the static `raku`.
- QEMU boots the nixpkgs kernel with an initramfs containing only the
  `raku` bundle and a static busybox, with a Raku script as `/init`. It
  prints `Rakudo 2025.06.1, PID 1`, `/proc mounted? False`,
  `vixen-mini PASS`. The compressed initrd is 17.9 MB.

Not done: NativeCall (impossible without `dlopen`), and embedding the
bytecode in the ELF to make one file.

## URLs consulted (rule 5)

- https://gist.githubusercontent.com/MostAwesomeDude/bb8cbfd005a33f5dd262d1f20a63a693/raw (challenge text)
- Prototype compiler gist: cloned once early in the session (contents never read); later requests were blocked by a local security hook. The user supplied the blog post as `ref/vixen.md`. The complete list, including every web-search result, is in `URLS.md`.
- https://lobste.rs/s/igpevt/lobsters_vibecoding_challenge_winter (no Task 2 technical discussion found)
- https://perl-foundation-outreach.github.io/ideas/perl6/linking.html
- https://github.com/moarvm/moarvm, https://github.com/raku/nqp, https://github.com/rakudo/rakudo (git clones at 2025.06 / 2025.06.1)
- nixpkgs `cff6bcc05680cea7406aa1f293d76be9c1483f02` (`pkgs/development/interpreters/rakudo/`)
- musl 1.2.5 tarball via nixpkgs `musl.src`
- Web searches: "Rakudo MoarVM static binary single executable musl", "MoarVM github issue static linking libmoar.a dynext perl6_ops", "Raku single file executable rakudo bundle bytecode embed", "Lobsters vibecoding challenge winter Raku static Vixen comments"

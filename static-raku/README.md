# Lobsters Vibecoding Challenge, Task 2: static Raku (S tier)

Task 2's S tier is "actually, Raku can be statically compiled and linked!".
This submission shows that it can. The patches and Nix flake here build a
fully static Rakudo (Raku) linked against musl, for `x86_64-linux` and
`aarch64-linux`. A flake check boots it as PID 1 in a bare initramfs and
runs a Vixen-style compiler on it.

The work was vibecoded with Claude Code (model: Opus 5.5).

## Files

| Path | What it is |
|------|------------|
| `README.md` | This file. |
| `static-raku/` | **The solution**, a Nix flake. See `static-raku/README.md`. |
| `static-raku/flake.nix`, `flake.lock` | Flake definition; nixpkgs pinned to `cff6bcc05680cea7406aa1f293d76be9c1483f02` (Rakudo 2025.06.1). |
| `static-raku/default.nix` | Packages: `moarvm`, `nqp`, `rakudo` (full static install) and `raku` (default: slim bundle with one static binary plus bytecode, 69.5 MiB, no references to other store paths). |
| `static-raku/patches/moarvm-builtin-rakudo-ops.patch` | Adds MoarVM `Configure.pl --builtin-rakudo-ops`: compiles Rakudo's extension ops into libmoar and resolves them from a built-in table instead of `dlopen`. |
| `static-raku/patches/rakudo-no-dynext.patch` | Stops Rakudo building and installing `dynext/libperl6_ops_moar.so`. |
| `static-raku/checks.nix` | Flake checks: `static-binaries`, `vixen-mini`, `initramfs-boot`. |
| `static-raku/tests/vixen-mini.raku`, `.expected` | Smoke test, a cut-down Vixen compiler. It uses the grammar verbatim from "An expression language for Vixen", plus actions, `Set` name analysis, execline output, and `callVixen` via `run`. |
| `static-raku/tests/initramfs-boot.nix` | Boots the nixpkgs kernel in QEMU with an initramfs holding only the `raku` bundle and a static busybox (`/bin/sh`), with a Raku script as `/init`. |
| `RESEARCH.md` | Why stock Rakudo can't be static (traced through MoarVM, NQP, Rakudo and musl sources), the experiments, and the result. |
| `URLS.md` | Every URL accessed, extracted from the full session transcript: fetches, clones, and every web-search query with all of its results. |
| `transcript.txt` | Chat log (Claude Code `/export`, condensed view). Redacted: local hook/tooling messages, an internal tool name, the username and the API provider. |

Not included: build logs and the throwaway diagnostic expression
(`nix/static-rakudo.nix`). `RESEARCH.md`'s evidence table cites `logs/` and
that file; the messages it quotes come from them.

## Usage

```
cd static-raku
nix build              # ./result/bin/raku, the slim static bundle
nix build .#rakudo     # full static Rakudo install (also .#nqp, .#moarvm)
nix flake check
```

## Time taken (wall clock)

From the session transcript's timestamps (UTC, 2026-11-16; local time
is UTC-8):

| Time (UTC) | Event | Elapsed |
|------------|-------|---------|
| 21:25:13 | First prompt: read the challenge, research why Raku can't be statically compiled | 0:00 |
| 22:04:00 | Root cause identified and confirmed with real static builds | 0:39 |
| 22:06:40 | Prompt: attempt the patch for the S tier | 0:41 |
| 23:13:44 | Static Raku working on both architectures; all six Linux checks pass | 1:48 |

Total: **1 hour 48 minutes**, much of it spent waiting for builds on a
remote Linux builder. Writing this submission came afterwards and isn't counted.

## `nix flake check` output

Run on 2026-11-16 in `static-raku/` with the files in this archive:

```
$ nix flake check --all-systems
evaluating flake...
checking flake output 'packages'...
checking derivation packages.x86_64-linux.default...
derivation evaluated to /nix/store/slz1qc2iql2v658xz4vy7vifqbkcrgrn-raku-static-2025.06.1.drv
checking derivation packages.x86_64-linux.raku...
derivation evaluated to /nix/store/slz1qc2iql2v658xz4vy7vifqbkcrgrn-raku-static-2025.06.1.drv
checking derivation packages.x86_64-linux.rakudo...
derivation evaluated to /nix/store/62d5d22pdqnpnx7b7qdmbng81mvk02ch-rakudo-static-2025.06.1.drv
checking derivation packages.x86_64-linux.nqp...
derivation evaluated to /nix/store/4b8mla6x02pz09anbphjp9m5vabv4fpb-nqp-static-2025.06.1.drv
checking derivation packages.x86_64-linux.moarvm...
derivation evaluated to /nix/store/h6wl1brblcxlmsbzq6ix3j5vaa6iinq5-moarvm-static-2025.06.drv
checking derivation packages.aarch64-linux.default...
derivation evaluated to /nix/store/i6dyccsh7ki49sf76rjqmmggvyb8wfxm-raku-static-2025.06.1.drv
checking derivation packages.aarch64-linux.raku...
derivation evaluated to /nix/store/i6dyccsh7ki49sf76rjqmmggvyb8wfxm-raku-static-2025.06.1.drv
checking derivation packages.aarch64-linux.rakudo...
derivation evaluated to /nix/store/qjan9y3a3q38pnljmy5cdh8aa2dvc1va-rakudo-static-2025.06.1.drv
checking derivation packages.aarch64-linux.nqp...
derivation evaluated to /nix/store/8xdb4b5a5m8k8q6j9fl71dlpm48skq1b-nqp-static-2025.06.1.drv
checking derivation packages.aarch64-linux.moarvm...
derivation evaluated to /nix/store/i447hx14jyvw5fd6wl5xlnkzhkndjzsx-moarvm-static-2025.06.drv
checking flake output 'checks'...
checking derivation checks.x86_64-linux.static-binaries...
derivation evaluated to /nix/store/rfl2zdvy1pfkmgpli8lhby5vfa9wv96z-static-raku-is-static.drv
checking derivation checks.x86_64-linux.vixen-mini...
derivation evaluated to /nix/store/02iqb3rfhkgsrad4j7sjqk27i7hywwys-static-raku-vixen-mini.drv
checking derivation checks.x86_64-linux.initramfs-boot...
derivation evaluated to /nix/store/1ylynq7wz5qkcm2h2zrqyvzj8zqv6fm0-static-raku-initramfs-boot.drv
checking derivation checks.aarch64-linux.static-binaries...
derivation evaluated to /nix/store/rq6ldaqscd4s33qhpnfjsd61nj6w9hqy-static-raku-is-static.drv
checking derivation checks.aarch64-linux.vixen-mini...
derivation evaluated to /nix/store/cwi1zhpw02pkbr22p85mm8vp38mzjfyv-static-raku-vixen-mini.drv
checking derivation checks.aarch64-linux.initramfs-boot...
derivation evaluated to /nix/store/c6kdcx9i6294g37sm2mmbrvn018ymmir-static-raku-initramfs-boot.drv
all checks passed!
```

That ran on an `aarch64-darwin` host. On a non-Linux host, `nix flake
check` only *evaluates* the Linux checks, so all six were also built on a
Linux builder from these same derivations:

```
$ nix build --no-link --print-out-paths .#checks.{x86_64,aarch64}-linux.{static-binaries,vixen-mini,initramfs-boot}
/nix/store/0k15ncpqj4pcg7bq5wr4j0z3iw7y90an-static-raku-is-static
/nix/store/fnvn691y1gh6a1im5hhyycc1crzb10p8-static-raku-vixen-mini
/nix/store/bhd9yif499pil6qk174f2ngl4wc7lmq4-static-raku-initramfs-boot
/nix/store/y5x5w50lwb4z12l3bh0pb97m7bgxq7ix-static-raku-is-static
/nix/store/a5sp9cd5ly4jym5c0wak1xx75rddkc7w-static-raku-vixen-mini
/nix/store/981i8b754k69vlzvb2p1hspwgk9paviq-static-raku-initramfs-boot
```

The `initramfs-boot` serial console shows, on both architectures:

```
STATIC-RAKU: Rakudo 2025.06.1, PID 1
STATIC-RAKU: /proc mounted? False
STATIC-RAKU: vixen-mini PASS
Kernel panic - not syncing: Attempted to kill init! exitcode=0x00000000
```

The panic is expected: `/init` exiting cleanly is how the check stops QEMU.

## Known limits

- NativeCall needs `dlopen`, so it doesn't work in a static binary. The
  Vixen compiler doesn't use it.
- The bundle is one static executable plus about 62 MiB of bytecode in
  `share/`, not a single file.
- Tested with a cut-down Vixen compiler (`tests/vixen-mini.raku`), not the
  full prototype compiler from the challenge gist.

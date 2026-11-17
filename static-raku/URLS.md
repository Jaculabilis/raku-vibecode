# URLs accessed (rule 5)

Extracted from the session's JSONL transcript (tool inputs and results).
Times are UTC on 2026-11-16.

## Fetched directly

| Time | Tool | URL | Outcome |
|------|------|-----|---------|
| 21:25:21 | shell (`curl`) | https://gist.githubusercontent.com/MostAwesomeDude/bb8cbfd005a33f5dd262d1f20a63a693/raw | Challenge text. Also fetched again at 21:26:08 and 21:26:31 (saved as `ref/challenge.md`). |
| 21:25:28 | shell (`git clone`) | https://gist.github.com/85f67849575fafb76becb6c9ebaa7604.git | Prototype compiler gist, cloned to `/tmp/gist-raku/src`. Reading its files was rejected by the user, so the contents were never viewed. |
| 21:26:31 | shell (`curl`) | https://api.github.com/gists/85f67849575fafb76becb6c9ebaa7604 | GitHub API rate limit exceeded; no content. |
| 21:26:38 | shell (`curl`) | https://gist.github.com/MostAwesomeDude/85f67849575fafb76becb6c9ebaa7604 | Blocked by a local security hook before running. |
| 21:27:18 | shell (`git clone`) | https://github.com/moarvm/moarvm.git (tag 2025.06), https://github.com/raku/nqp.git (2025.06.1), https://github.com/rakudo/rakudo.git (2025.06.1) | Sources studied and patched. |
| 21:27:18 | (git submodules of the above) | https://github.com/libuv/libuv.git, https://github.com/MoarVM/dyncall.git, https://github.com/MoarVM/dynasm.git, https://github.com/MoarVM/libtommath, https://github.com/MoarVM/cmp.git, https://github.com/MoarVM/libatomic_ops, https://github.com/MoarVM/ryu, https://github.com/microsoft/mimalloc.git, https://github.com/Nicoshev/rapidhash, https://github.com/Raku/nqp-configure.git | Fetched by `--recurse-submodules`. |
| 21:41:09 | WebFetch | https://perl-foundation-outreach.github.io/ideas/perl6/linking.html | TPF idea "Linking Perl 6 Compiler and Programs". |
| 21:41:26 | WebFetch | https://lobste.rs/s/igpevt/lobsters_vibecoding_challenge_winter | Challenge thread; no Task 2 technical discussion. |

## Web searches (query, then every result returned)

Only the results marked **opened** were fetched. The rest were returned as
titles and URLs and never visited.

1. *Rakudo MoarVM static binary single executable musl* (21:40:57)
   - https://en.wikipedia.org/wiki/MoarVM
   - https://www.sigplan.org/OpenTOC/vmil21.html
   - https://SourceForge.Net/projects/moarvm.mirror/
   - https://www.graalvm.org/22.2/reference-manual/native-image/guides/build-static-executables/
   - https://www.moarvm.com/index.html
   - https://qa.rustfaq.org/en/how-to-target-musl-for-fully-static-linux-binaries/
   - https://highassurance.rs/chp2/operational_assurance_2.html
   - https://docs.oracle.com/en/graalvm/jdk/21/docs/reference-manual/native-image/guides/build-static-executables/
   - https://www.graalvm.org/jdk21/reference-manual/native-image/guides/build-static-executables/
   - https://gist.github.com/jnthn/7843dadbe45e3f56f02f6a679c21f407
2. *MoarVM github issue static linking libmoar.a dynext perl6_ops* (21:40:57)
   - https://www.mail-archive.com/debian-bugs-rc@lists.debian.org/msg540067.html
   - https://www.mail-archive.com/debian-bugs-dist@lists.debian.org/msg1309649.html
   - https://perl-foundation-outreach.github.io/ideas/perl6/linking.html (**opened**)
   - https://gist.github.com/jnthn/7843dadbe45e3f56f02f6a679c21f407
   - https://www.mail-archive.com/perl6-compiler@perl.org/msg09241.html
   - https://www.funwithlinux.net/blog/pack-shared-libraries-into-the-elf/
   - https://www.mail-archive.com/automake@gnu.org/msg10953.html
   - https://www.mail-archive.com/digitalmars-d@puremagic.com/msg21590.html
   - https://community.intel.com/t5/Intel-Fortran-Compiler/Using-dlopen-in-statically-linked-applications-requires-at/td-p/1298834
   - https://community.intel.com/t5/Intel-Fortran-Compiler/linking-with-openmp-static-lib/m-p/916521
3. *Raku "single file" executable rakudo bundle bytecode embed patrickb* (21:41:10)
   - https://docs.huihoo.com/racket/7.6/raco/exe.html
   - https://mirror.racket-lang.org/docs/7.7/html/raco/exe.html
   - https://www-old.cs.utah.edu/plt/popl16/doc/raco/exe.html
   - https://mirror.racket-lang.org/docs/5.3.4/html/raco/exe.html
   - https://docs.racket-lang.org/raco/exe.html
4. *Lobsters vibecoding challenge winter Raku static Vixen comments* (21:41:10)
   - https://gist.github.com/MostAwesomeDude/bb8cbfd005a33f5dd262d1f20a63a693?permalink_comment_id=5986825
   - https://testbook.com/question-answer/if-summer-is-coded-as-runner--677bed141d856333e117fba4/amp
   - https://www.nerdterms.com/terms/vibe-coding
   - https://testbook.com/question-answer/in-a-certain-code-language-if-winter-is-coded-as--5fb392cdfb1474c29fc819c6
   - https://lobste.rs/s/u0nv8q/diffshub
   - https://testbook.com/question-answer/in-a-certain-code-language-frolic-is-written-as-r--6246f11b9a6812c388d09424
   - https://teckhustlers.com/what-is-vibe-coding/
   - https://lobste.rs/s/djzqbn/can_we_fix_retire_vibecoding_tag
   - https://www.restaurantbusinessonline.com/technology/vibe-coding-changing-build-or-buy-debate-restaurants
   - https://shows.acast.com/learn-french-la-pause-cafe-croissant/episodes/quest-ce-que-le-vibecoding-learn-french-podcast

## Fetched indirectly by Nix

These aren't individual URLs in the transcript, but Nix accessed them:

- `github:nixos/nixpkgs/cff6bcc05680cea7406aa1f293d76be9c1483f02`, the
  pinned nixpkgs flake input. It was fetched from GitHub on first use (first
  at 21:42:29).
- https://cache.nixos.org, binary substitutes for toolchains, the kernel,
  QEMU, busybox and the stock `rakudo`/`nqp`/`moarvm` sources. Nix logs each
  path it copies from here (see `logs/`). Anything not in the cache was
  built on the local Linux builder (`ssh-ng://builder@linux-builder`), which
  is not an internet URL.

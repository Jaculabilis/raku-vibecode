#!/usr/bin/env python3
"""Mutation fuzzing of the bytecode loader, verifier and VM.

Compiles every single-file test case, then repeatedly corrupts the
bytecode (random byte flips, truncations, operand rewrites) and runs it with
menagerie-vm.  The VM may reject the file or fail with a runtime error, but
it must never crash: a Rust panic or death by signal is a failure.  Runs
that time out are fine (a mutation can create an infinite loop).

usage: fuzz_bytecode.py BIN_DIR CASES_DIR [ITERATIONS]
"""

import os
import random
import subprocess
import sys
import tempfile

EXTS = (".lox", ".monkey", ".tig")


def main():
    bindir, cases = sys.argv[1], sys.argv[2]
    iterations = int(sys.argv[3]) if len(sys.argv) > 3 else 500
    compiler = os.path.join(bindir, "menageriec")
    vm = os.path.join(bindir, "menagerie-vm")
    rng = random.Random(1234)
    seeds = []
    with tempfile.TemporaryDirectory() as tmp:
        for root, _, files in os.walk(cases):
            for f in sorted(files):
                if not f.endswith(EXTS):
                    continue
                out = os.path.join(tmp, f"seed{len(seeds)}.mbc")
                r = subprocess.run([compiler, "-o", out, os.path.join(root, f)], capture_output=True)
                if r.returncode == 0:
                    seeds.append(open(out, "rb").read())
        if not seeds:
            sys.exit("no seed programs compiled")
        crashes = 0
        target = os.path.join(tmp, "mutant.mbc")
        for i in range(iterations):
            data = bytearray(rng.choice(seeds))
            kind = rng.randrange(4)
            if kind == 0:
                for _ in range(rng.randint(1, 4)):
                    data[rng.randrange(len(data))] = rng.randrange(256)
            elif kind == 1:
                del data[rng.randrange(len(data)):]
            elif kind == 2:
                # Rewrite a 32-bit little-endian field with a boundary value.
                pos = rng.randrange(max(1, len(data) - 4))
                val = rng.choice([0, 1, 2, 255, 0x7FFFFFFF, 0xFFFFFFFF, rng.randrange(64)])
                data[pos:pos + 4] = val.to_bytes(4, "little")
            else:
                pos = rng.randrange(len(data))
                data[pos:pos] = bytes(rng.randrange(256) for _ in range(rng.randint(1, 8)))
            with open(target, "wb") as f:
                f.write(data)
            try:
                r = subprocess.run([vm, target], capture_output=True, timeout=5, stdin=subprocess.DEVNULL)
            except subprocess.TimeoutExpired:
                continue
            stderr = r.stderr.decode(errors="replace")
            if r.returncode < 0 or "panicked" in stderr or "internal error" in stderr:
                crashes += 1
                keep = f"crash-{i}.mbc"
                with open(keep, "wb") as f:
                    f.write(data)
                print(f"CRASH (iteration {i}, saved {keep}): exit {r.returncode}: {stderr.strip()[:300]}")
        print(f"fuzz: {iterations} mutants of {len(seeds)} programs, {crashes} crashes")
        sys.exit(1 if crashes else 0)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Run the Crafting Interpreters Lox test suite against menagerie.

Re-implements the expectation rules of the book's tool/bin/test.dart for the
"jlox" suite: `// expect: ...` output lines, `// Error ...` and
`// [line N] Error ...` compile errors (exit 65), and
`// expect runtime error: ...` (exit 70, message then "[line N]").

usage: run_lox_suite.py MENAGERIEC SUITE_DIR [-v]
"""

import os
import re
import subprocess
import sys

EXPECT_OUT = re.compile(r"// expect: ?(.*)")
EXPECT_ERR = re.compile(r"// (Error.*)")
EXPECT_ERR_LINE = re.compile(r"// \[((java|c) )?line (\d+)\] (Error.*)")
EXPECT_RUNTIME = re.compile(r"// expect runtime error: (.+)")
SYNTAX_ERR = re.compile(r"\[.*line (\d+)\] (Error.+)")
STACK_LINE = re.compile(r"\[line (\d+)\]")
NONTEST = re.compile(r"// nontest")

# Same exclusions as the book's jlox suite.  nan_equality is skipped there
# only because the JVM's boxed doubles are not IEEE; we are, so we run it
# with clox's expectations.
SKIP = {
    "limit/loop_too_large.lox",
    "limit/no_reuse_constants.lox",
    "limit/too_many_constants.lox",
    "limit/too_many_locals.lox",
    "limit/too_many_upvalues.lox",
    "limit/stack_overflow.lox",
}


def parse(path):
    out, errors, runtime, runtime_line, code = [], set(), None, 0, 0
    with open(path, encoding="utf-8") as f:
        for n, line in enumerate(f.read().splitlines(), 1):
            if NONTEST.search(line):
                return None
            m = EXPECT_OUT.search(line)
            if m:
                out.append(m.group(1))
                continue
            m = EXPECT_ERR.search(line)
            if m:
                errors.add(f"[{n}] {m.group(1)}")
                code = 65
                continue
            m = EXPECT_ERR_LINE.search(line)
            if m:
                if m.group(2) in (None, "java"):
                    errors.add(f"[{m.group(3)}] {m.group(4)}")
                    code = 65
                continue
            m = EXPECT_RUNTIME.search(line)
            if m:
                runtime, runtime_line, code = m.group(1), n, 70
    return out, errors, runtime, runtime_line, code


def run_one(compiler, path):
    spec = parse(path)
    if spec is None:
        return None
    out, errors, runtime, runtime_line, code = spec
    try:
        r = subprocess.run([compiler, "-r", path], capture_output=True, text=True, timeout=60)
    except subprocess.TimeoutExpired:
        return ["timed out"]
    fails = []
    err_lines = r.stderr.splitlines()
    if runtime is not None:
        if len(err_lines) < 2:
            fails.append(f"expected runtime error {runtime!r}, got {err_lines}")
        else:
            if err_lines[0] != runtime:
                fails.append(f"expected runtime error {runtime!r}, got {err_lines[0]!r}")
            lines = [STACK_LINE.search(l) for l in err_lines[1:]]
            lines = [m for m in lines if m]
            if not lines or int(lines[0].group(1)) != runtime_line:
                fails.append(f"expected runtime error on line {runtime_line}, stderr {err_lines[1:]}")
    else:
        found = set()
        for l in err_lines:
            m = SYNTAX_ERR.search(l)
            if m:
                e = f"[{m.group(1)}] {m.group(2)}"
                if e in errors:
                    found.add(e)
                else:
                    fails.append(f"unexpected error: {l}")
            elif l:
                fails.append(f"unexpected output on stderr: {l}")
        for e in sorted(errors - found):
            fails.append(f"missing expected error: {e}")
    if r.returncode != code:
        fails.append(f"expected exit code {code}, got {r.returncode}")
    got = r.stdout.splitlines()
    for i, want in enumerate(out):
        if i >= len(got):
            fails.append(f"missing output line {i + 1}: {want!r}")
            break
        if got[i] != want:
            fails.append(f"output line {i + 1}: expected {want!r}, got {got[i]!r}")
    if len(got) > len(out):
        fails.append(f"unexpected extra output: {got[len(out):][:3]}")
    return fails


def main():
    compiler, suite = sys.argv[1], sys.argv[2]
    verbose = "-v" in sys.argv
    passed = failed = skipped = 0
    for root, _, files in sorted(os.walk(suite)):
        for name in sorted(files):
            if not name.endswith(".lox"):
                continue
            path = os.path.join(root, name)
            rel = os.path.relpath(path, suite)
            if rel in SKIP:
                skipped += 1
                continue
            fails = run_one(compiler, path)
            if fails is None:
                continue
            if fails:
                failed += 1
                print(f"FAIL {rel}")
                for f in fails[: (20 if verbose else 4)]:
                    print(f"    {f}")
            else:
                passed += 1
    print(f"lox suite: {passed} passed, {failed} failed, {skipped} skipped")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()

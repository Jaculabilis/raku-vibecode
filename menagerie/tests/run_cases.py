#!/usr/bin/env python3
"""Golden tests for menagerie.

Each test is a source file under CASES_DIR, or a directory holding sources
in more than one language, whose files are linked together in sorted name
order (a polyglot program).  Expectations
are written in comments anywhere in the sources:

    expect: TEXT                 next line of standard output
    expect error: TEXT           compilation fails (exit 65) and stderr
                                 contains TEXT
    expect runtime error: TEXT   first stderr line is TEXT; exit 70
    expect exit: N               exit status N

(`// expect: ...` in Lox and Monkey, `/* expect: ... */` in Tiger.)  A file
NAME.in next to a test (or `stdin` inside a test directory) is fed to
standard input.  Trailing whitespace on output lines is ignored.  Each test
is run twice: compiled and run in one step, and
compiled to a .mbc file that is then run by menagerie-vm.

usage: run_cases.py BIN_DIR CASES_DIR...
"""

import os
import re
import subprocess
import sys
import tempfile

SOURCE_EXT = (".lox", ".monkey", ".mon", ".mky", ".tig", ".tiger", ".mbc")
EXPECT = re.compile(r"expect(?: (error|runtime error|exit))?: ?(.*?)\s*(?:\*/)?\s*$")


def expectations(files):
    out, spec = [], {"error": None, "runtime": None, "exit": 0}
    for path in files:
        with open(path, encoding="utf-8") as f:
            for line in f:
                i = line.find("expect")
                if i < 0 or not re.search(r"(//|/\*)\s*$", line[:i]):
                    continue
                m = EXPECT.match(line[i:].rstrip("\n"))
                if not m:
                    continue
                kind, text = m.group(1), m.group(2)
                if kind is None:
                    out.append(text)
                elif kind == "error":
                    spec["error"] = text
                    spec["exit"] = 65
                elif kind == "runtime error":
                    spec["runtime"] = text
                    spec["exit"] = 70
                else:
                    spec["exit"] = int(text)
    return out, spec


def check(result, out, spec, how):
    fails = []
    err = result.stderr.splitlines()
    if result.returncode != spec["exit"]:
        fails.append(f"{how}: exit {result.returncode}, expected {spec['exit']}; stderr: {err[:3]}")
    if spec["error"] is not None and spec["error"] not in result.stderr:
        fails.append(f"{how}: expected compile error containing {spec['error']!r}, got {err[:3]}")
    if spec["runtime"] is not None and (not err or err[0] != spec["runtime"]):
        fails.append(f"{how}: expected runtime error {spec['runtime']!r}, got {err[:2]}")
    if spec["error"] is None and spec["runtime"] is None and result.returncode == spec["exit"] and err:
        fails.append(f"{how}: unexpected stderr: {err[:3]}")
    got = [l.rstrip() for l in result.stdout.splitlines()]
    if got != out:
        for i in range(max(len(got), len(out))):
            g = got[i] if i < len(got) else "<missing>"
            w = out[i] if i < len(out) else "<nothing>"
            if g != w:
                fails.append(f"{how}: stdout line {i + 1}: expected {w!r}, got {g!r}")
                break
    return fails


def run_test(bindir, files, stdin_path):
    out, spec = expectations(files)
    stdin = open(stdin_path, "rb").read() if stdin_path else b""
    run = lambda cmd: subprocess.run(cmd, input=stdin, capture_output=True, timeout=120)
    decode = lambda r: subprocess.CompletedProcess(r.args, r.returncode, r.stdout.decode(), r.stderr.decode())
    compiler = os.path.join(bindir, "menageriec")
    vm = os.path.join(bindir, "menagerie-vm")
    fails = check(decode(run([compiler, "-r", *files])), out, spec, "menageriec -r")
    with tempfile.TemporaryDirectory() as tmp:
        mbc = os.path.join(tmp, "prog.mbc")
        c = decode(run([compiler, "-o", mbc, *files]))
        if spec["error"] is not None:
            fails += check(c, out, spec, "menageriec -o")
        elif c.returncode != 0:
            fails.append(f"menageriec -o: exit {c.returncode}: {c.stderr.splitlines()[:3]}")
        else:
            fails += check(decode(run([vm, mbc])), out, spec, "menagerie-vm")
    return fails


def collect(root):
    tests = []
    files = sorted(os.path.join(root, f) for f in os.listdir(root) if f.endswith(SOURCE_EXT))
    if len({os.path.splitext(f)[1] for f in files}) > 1:
        stdin = os.path.join(root, "stdin")
        return [(root, files, stdin if os.path.exists(stdin) else None)]
    for entry in sorted(os.listdir(root)):
        path = os.path.join(root, entry)
        if os.path.isdir(path):
            tests += collect(path)
        elif entry.endswith(SOURCE_EXT):
            stdin = os.path.splitext(path)[0] + ".in"
            tests.append((path, [path], stdin if os.path.exists(stdin) else None))
    return tests


def main():
    bindir, roots = sys.argv[1], sys.argv[2:]
    passed = failed = 0
    for root in roots:
        for name, files, stdin in collect(root):
            fails = run_test(bindir, files, stdin)
            if fails:
                failed += 1
                print(f"FAIL {name}")
                for f in fails[:6]:
                    print(f"    {f}")
            else:
                passed += 1
    print(f"cases: {passed} passed, {failed} failed")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()

# Menagerie

Menagerie compiles three teaching languages into one common bytecode:

* **Lox**, from Robert Nystrom's *Crafting Interpreters*
* **Monkey**, from Thorsten Ball's *Writing An Interpreter In Go*,
  *Writing A Compiler In Go* and *The Lost Chapter* (macros)
* **Tiger**, from Andrew Appel's *Modern Compiler Implementation*, as
  specified by Stephen Edwards' *Tiger Language Reference Manual*

It also ships a reference interpreter for that bytecode. Files in different
languages can be linked into **one program** that calls back and forth
between the languages.

```
$ nix build                          # builds bin/menageriec and bin/menagerie-vm
$ cd examples/polyglot
$ ../../result/bin/menageriec -r 0-map.monkey 1-numbers.tig 2-greeter.lox 3-main.monkey
Hello, menagerie~~~
Greeter instance
0 1 1 2 3 5 8 13 21 34 55 89
three languages, one bytecode!
wow!!
[55, 6765, 832040]
```

In that program, Monkey calls a Lox class and a Lox function. The Lox
function calls Tiger's `fib` and `repeat`, and a Tiger procedure calls back
into a Lox function that it declares as a `primitive`.

## Usage

```
menageriec [-o OUT.mbc] [-x LANG] [-S] [-r] FILE...
menagerie-vm [-S] FILE.mbc...
```

`menageriec` compiles each source file and links the results into one
program; files that end in `.mbc` are linked in as they are. It writes
`out.mbc` unless `-o` names another file. `-r` runs the program instead of
writing it (pass `-o` as well to do both), and `-S` prints a disassembly.
The language comes from the file extension (`.lox`; `.monkey`, `.mon` or
`.mky`; `.tig` or `.tiger`), or from `-x lox|monkey|tiger`.

`menagerie-vm` is the reference interpreter. It loads, links and verifies
one or more bytecode files, then runs their modules in order.

Exit status: 0 on success, 65 for a compile error or invalid bytecode, 70
for a runtime error (jlox's codes), 64 for a usage error. Tiger's
`exit(n)` exits with `n`.

```
$ nix run .#menageriec -- -o fib.mbc fib.tig      # compile
$ nix run .#menagerie-vm -- fib.mbc               # interpret
$ nix flake check                                 # build and run every test
$ nix develop                                     # cargo, rustc, clippy, python3
```

## The languages

### Lox

The complete language: classes, inheritance, `super`, initializers,
closures, `for`, `while`, `print`, and the native `clock()`. Semantics,
diagnostics and exit codes follow jlox, including the resolver's static
errors. On the book's official test suite (vendored in `tests/lox-suite`,
MIT licence), **240 of 240** applicable tests pass. Following the book's own
jlox runner, we skip six `limit/` tests that only apply to clox's fixed
limits. We do run `nan_equality.lox`, which jlox skips; we use clox's IEEE
expectations for it.

Small differences from jlox: numbers of 10^21 and above print as `1e21`
rather than Java's `1.0E21`, and deep recursion stops with `Stack overflow.`
instead of a JVM `StackOverflowError`.

### Monkey

The whole language from both books: integers, booleans, strings, arrays,
hashes, first-class functions and closures, `if`/`else` expressions,
`return`, and the builtins `len`, `first`, `last`, `rest`, `push` and `puts`.
It also implements *The Lost Chapter*: `quote`/`unquote` and `macro`.
Macros expand at compile time, which needs an evaluator, so the compiler
contains a small Monkey tree-walker for macro bodies
(`src/frontend/monkey_macro.rs`). The parser is a transcription of the
book's Pratt parser and reproduces its error messages and recovery exactly.

The books' evaluator and compiler disagree about scoping. We follow the
evaluator, which is the language's reference semantics:

* There is one environment per call, and blocks don't create scopes.
* `let` binds or rebinds a name in the current environment.
* Closures share their defining environment, so they see later rebindings.
* A name that is not yet bound is looked up in the enclosing environment.

The compiler resolves this statically, except for the question "has this
`let` run yet?". It answers that at run time with the
`GetGlobalOpt`/`JumpIfNotUndef` instructions (see
`tests/cases/monkey/scoping.monkey`).

Deviations: Go's random hash iteration order is replaced by insertion
order. `==` on strings compares contents; the book leaves this as an
exercise and its interpreter reports an error. `/` by zero is a runtime
error, not a Go panic. Extensions: string escapes (`\n \t \r \" \\`), `//`
line comments, and digits in identifiers after the first character.
Runtime errors print as `ERROR: message`, the way the book's REPL shows
error objects.

### Tiger

The whole language as specified in Edwards' manual:

* records, arrays and type aliases, with mutually recursive types and
  functions
* nested functions with static scoping
* `let`, `for`, `while`, `break` and lazy `&`/`|`
* string comparison
* all string escapes, including `\^c`, `\ddd` and `\...\`
* nested comments
* the full standard library (`print`, `printi`, `flush`, `getchar`, `ord`,
  `chr`, `size`, `substring`, `concat`, `not`, `exit`)

There is a complete type checker. On Appel's 49 test cases from the book's
web site, every program marked "error" is rejected with a relevant message,
and every valid one compiles and runs. Two of the valid ones, test6 and
test7, recurse forever by design, so they stop with a stack overflow.
`queens.tig` and `merge.tig` produce the right output. The test cases are
not vendored because they carry no licence. `tests/cases/tiger` contains
adapted copies of `queens` and `merge`, plus our own tests.

Integers are 64-bit. Runtime errors (nil record access, array bounds,
division by zero, `chr` and `substring` ranges) stop the program with a
message and line number.

## Programs in several languages

All modules share one global namespace:

* **Lox**: top-level `var`, `fun` and `class` declarations are globals,
  and unresolved names are looked up there at run time.
* **Monkey**: top-level `let`s are globals, and free names fall back to
  the globals (and then to the builtins) at run time.
* **Tiger**: functions declared in the program's outermost `let` are
  exported as globals. A `primitive` declaration, borrowed from the EPITA
  Tiger dialect, imports a function by name with a type signature:

  ```
  let primitive shout(s: string): string
  in print(shout("hi")) end
  ```

  Tiger checks values crossing its boundary: an `int` or `string` result
  from a primitive, and the arguments of exported functions. Integral floats
  from Lox become ints; anything else is a runtime error.

Modules run in command-line order. Thanks to late binding, an earlier
module's functions may call a later module's functions once those exist.
Values keep their identity across languages, and each language prints
foreign values sensibly: Lox prints a Monkey array as `[1, 2]`, and Monkey
prints a Lox instance as `Point instance`. Lox can read and write a Tiger
record's fields as properties. Lox numbers are floats and Monkey/Tiger
numbers are ints, so arithmetic on a mix gives a float. A runtime error is
reported in the style of the language whose code failed. See
`tests/cases/polyglot` for worked examples.

## How it works

* `src/frontend/{lox,monkey,tiger}.rs`: one lexer, parser, checker and code
  generator per language. Lox runs a jlox-style resolver pass. Tiger's type
  checker produces a typed tree that the code generator walks. Tiger
  expressions of type unit leave nothing on the stack.
* `src/emit.rs`: shared code generation, made of a function builder (labels,
  static stack-height tracking) and a resolver for captured variables. A
  variable captured by an inner function lives in a heap cell; all others
  stay in plain stack slots.
* `src/bytecode.rs`: the instruction set, the binary file format, the
  linker, the verifier and the disassembler. **See [BYTECODE.md](BYTECODE.md)
  for the specification.**
* `src/vm.rs`, `src/value.rs`, `src/natives.rs`: the interpreter. Frames
  live on the heap, so recursion depth isn't limited by the native stack
  (the limit is 200,000 frames).

The interpreter verifies every program before running it, and treats
malformed input as an error, never a crash. The code contains no `unsafe`.
`tests/fuzz_bytecode.py` mutates compiled programs to check this.

Memory is reference-counted, so cyclic garbage (such as a closure that
refers to itself) is not reclaimed until the program exits. For the
short-lived programs these languages are used for, that is a deliberate
simplification.

## Tests

`nix flake check` runs everything below; the commands also work from a
`nix develop` shell after `cargo build --release`.

* `cargo test`: unit tests for the encoding, the linker and the verifier
* `tests/run_lox_suite.py target/release/menageriec tests/lox-suite`:
  the official Lox suite, using the book's expectation rules
* `tests/run_cases.py target/release tests/cases examples`: golden tests
  for Monkey, Tiger, Lox extras and polyglot programs. Each test runs both
  through `menageriec -r` and as a `.mbc` file through `menagerie-vm`.
* `tests/fuzz_bytecode.py target/release tests/cases 1500`: bytecode
  mutation fuzzing

## Tools used

The code was written by Claude Opus 5.5 (`claude-opus-5-5`), working in the
Claude Code harness. The work was prompted from the challenge text in
`five-feathers.md`. Reference material: Edwards' Tiger manual (PDF), Appel's
Tiger test cases, and the Crafting Interpreters repository (for its test
suite and test-runner rules).

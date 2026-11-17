# META Restricted ACE Zaddy

Zaddy is a language for describing compilers.  A Zaddy program describes a
pipeline:

1. a **grammar** parses UTF-8 text into terms of one or more tree algebras;
2. **rewrite rules** saturate an e-graph containing those terms, translating
   between algebras modulo associativity, commutativity and equality (ACE);
3. **extraction** picks, for the root of the e-graph, the cheapest term of the
   builtin line/block algebra and prints it.

The reference implementation compiles a Zaddy program to an RPython program.
That program is the compiler described by the Zaddy program.  `zaddy.zaddy`
describes Zaddy itself, and `rzaddy/zaddy.py` is the result of compiling
`zaddy.zaddy` with itself: a fixed point.

    zaddyc < program.zaddy > program.py       # Zaddy -> RPython
    rpython program.py                        # -> native binary, e.g. bfc
    bfc < hello.b > hello.c

## Command line

Every compiler produced by Zaddy, including `zaddyc`, has the same interface:

    NAMEc [-v] [-i ITERATIONS] [-n NODES] < input > output

* input is read from stdin and must be UTF-8;
* output is written to stdout as UTF-8 (see *Output format*);
* `-v` prints per-iteration statistics and per-rule application counts to
  stderr;
* `-i` limits the number of saturation iterations (default 1000);
* `-n` stops saturation once the e-graph holds more than NODES e-nodes
  (default 1000000).  Output is still extracted from the partial e-graph.

Exit status: 0 on success, 1 on a parse error, 2 on bad arguments, 3 if no
output could be extracted or a rule referenced an unbound variable.  Parse
errors print a trail of the furthest rule matches with line:column ranges.
Extraction failures list the constructors that were never rewritten into
something printable, which almost always identifies the missing rule.

## Input format: the Zaddy language

A Zaddy file is a sequence of sections.  Whitespace separates tokens.  A
comment starts with `--` and runs to the end of the line.  Comments may
appear between sections and between the items of a section (between
signature types, rules, and grammar productions).

    file      := ( signature | rules | grammar | comment )*

### Signatures

    .signature NAME
    type = Con1(fieldtype name, ...) | Con2 | Con3(...) .ac | ...

A signature declares a namespaced tree algebra in the style of Zephyr ASDL.
Each constructor `Con` of signature `ns` becomes a term former `ns.Con`
usable in rules and grammar actions.  Field types are documentation only;
fields may be written `type name`, `type? name` or `type* name`.  ASDL
product types (`type = (fields)`) are accepted and ignored.  A type may
end with `attributes (fields)`, which is also ignored.

A constructor may carry one equational annotation:

| annotation | meaning                         | storage and matching            |
|------------|---------------------------------|---------------------------------|
| (none)     | free constructor                | arguments matched positionally  |
| `.comm`    | commutative                     | arguments kept sorted; patterns match any permutation |
| `.assoc`   | associative                     | nested applications flattened to n-ary sequences; patterns split them into contiguous runs |
| `.ac`      | associative and commutative     | flattened to n-ary multisets; patterns split them into sub-multisets |

Signatures may be split over several `.signature` sections with different
names.  The `builtin` signature is predefined:

    .signature builtin
    out = Line(string s) | Block(out* body)

### Rules

    .rules
    name @ pattern ==> production, production, ... ;   -- propagation
    name @ pattern <=> production, production, ... ;   -- simplification
    let name := production ;                           -- named constant

A rule fires on every e-class matching its pattern and merges the class with
each of its productions.  A `==>` rule keeps the matched e-node.  A `<=>`
rule also *subsumes* it: the matched e-node remains in the e-graph for
hashconsing but is never matched or extracted again.  This is CHR's
simplification, or egglog's `subsume`.  Use it for rewrites that strictly
replace a term, such as peephole optimizations of long sequences.  It stops
the e-graph from keeping every intermediate version.

Patterns:

| syntax              | matches                                                    |
|---------------------|------------------------------------------------------------|
| `X`                 | any e-class; binds `X`.  A variable repeated in one pattern must bind the same e-class (non-linear matching) |
| `_`                 | any e-class                                                |
| `'text'`            | the e-class containing the string `text`                   |
| `ns.Con(p, ...)`    | an e-class containing an `ns.Con` e-node whose arguments match, modulo the constructor's annotation |
| `ns.Con`            | a nullary constructor                                      |
| `[]`                | an e-class containing the empty list                       |
| `[p, Xs…, q, ...]`  | an e-class containing a list matching the items in order.  `Xs…` (or `Xs...`) binds any run of zero or more elements, so `[Xs…, a, b, Ys…]` finds `a, b` anywhere in the list (matching modulo associativity of list concatenation) |

Patterns nest to any depth, for example
`bf.Loop([bf.Minus('1'), bf.Right(N), bf.Plus(K), bf.Left(N)])`.  The root of
a pattern must be a constructor, a list, or a string.

When an AC or associative constructor is matched with fewer pattern arguments
than the term has, an argument that is a variable (or `_`, or the same
constructor) absorbs several elements.  The absorbed elements are
materialized as a new e-node.  In addition, a top-level AC or associative
pattern may match any sub-multiset (or sub-run) of a larger term.  This is
*matching with extension*, as in Maude.  For example,
`ops.Add(X, ops.Neg(X))` fires on `x + y + -x`: the sub-term `x + -x` is
materialized and merged with the rule's result, and the original term is
linked to `y + (x + -x)`.

Productions:

| syntax                | builds                                              |
|-----------------------|-----------------------------------------------------|
| `X`                   | the e-class bound to `X`                            |
| `name`                | a `let` constant (lower-case identifier)            |
| `'text'`              | a string                                            |
| `NNN`                 | the one-character string with code point NNN        |
| `p q r`               | string concatenation of adjacent productions        |
| `ns.Con(p, ...)`      | a constructor term                                  |
| `[p, Xs…, q]`         | a list; `Xs…` splices the elements bound to `Xs`    |
| `*p`, `*(p)`          | flatten: concatenate a list of lists.  A one-element list flattens to its element |
| `#p`                  | the length of a string, as a decimal string         |
| `.join(p, 'sep')`     | join a list of strings with a separator             |
| `.line(p)`, `.block(p)` | `builtin.Line` and `builtin.Block`                |
| `.gensym`, `.gensym('prefix')` | a fresh string `prefix1`, `prefix2`, ...  |

Concatenation, flattening, length and join are evaluated eagerly whenever
their arguments are available in the e-graph.  String builtins use the
shortest (then lexicographically least) string of each argument class.

### Grammars

    .grammar NAME
    class Name = charset ;
    token Name = Class Class* Class+ ... ;
    RULE := parsing expression ;

A grammar is a packrat PEG parser.  Parsing starts at the rule with the same
name as the grammar.  The whole input must be consumed.

* `class` declares a character set: a decimal code point, `.range(lo:hi)`,
  `.any`, another class name, `~set` (complement), `a | b` (union), and
  parentheses.
* `token` declares a scanner: a sequence of classes, each optionally followed
  by `*` or `+`.  A token's value is the string it scanned.  Tokens do not
  skip whitespace.
* `RULE := e ;` declares a parsing rule.  Expressions are `'literal'` (skips
  whitespace 9/10/13/32 before matching), rule or token names,
  `( e )`, `e*`, `e+`, `e?`, `&e`, `!e`, sequence by juxtaposition, ordered
  choice `e / e`, capture `e:Name`, and production `e -> production`.  The
  production language is the same as in rules.  Captured names are its
  variables.  `e*` and `e+` produce lists.

The value of the start rule is the root of the e-graph.  It is wrapped in a
flatten, so it should be a list whose elements rewrite to lists of builtins.

## Output format

After saturation, the root e-class must contain a list whose elements each
contain a `builtin.Line` or `builtin.Block`.  Extraction chooses the cheapest
such term, where the cost is the number of e-nodes in the extracted tree.
Ties between strings go to the shortest, then lexicographically least.
Output is printed as:

* `Line(s)`: the string `s` on its own line, indented by four spaces per
  enclosing block;
* `Block(body)`: the lines of `body`, indented one level deeper.

Lines are joined with `\n`, followed by a final newline, and encoded as UTF-8.
Nothing else is written to stdout.

For `zaddyc` the output is an RPython module.  It defines `target`, so it can
be translated with `rpython`, and `main`, so it can also be run untranslated
on Python 2.7.  The module contains, in order:

1. the runtime: e-graph, matcher, builtin evaluation, extraction, `main`;
2. for each signature `ns`, a class `nsRels` with one `makeCon` method per
   constructor;
3. for each rule, a function `rhsNAME(uf, rel, env)` returning the e-classes
   of its productions, registered with its pattern (built from `PStruct`,
   `PList`, `PSplice`, `PVar`, `PStr` and `PIgnore`) in `ruleList`;
4. for each grammar, a parser class with one method per class, token and
   rule.

## Semantics

Saturation repeats until nothing changes (or a limit is hit):

1. **rebuild**: re-canonicalize every e-node.  Hashcons collisions are merged
   (congruence closure), commutative arguments are re-sorted, and an AC or
   associative e-node whose argument class now contains a smaller application
   of the same constructor is merged with the flattened e-node (only if that
   e-node already exists, so that rebuild cannot grow the e-graph);
2. **match** every rule against the e-graph, collecting all matches first;
3. **evaluate** builtin flatten, length, join and concatenation;
4. **apply** every match, merging e-classes and subsuming `<=>` matches.

Equality is everywhere.  Repeated variables, string patterns and splices
compare e-classes, not syntax, so a rule that matches `X + X` also matches
`a * b + b * a` and `x * (y + 0) + x * y` once `y + 0` has been merged with `y`.

## ACE matching in one example

`arith.zaddy` declares `Add` and `Mul` as `.ac` and simplifies one
expression per line:

    $ echo 'x * (y + 0) + y * x;' | arithc
    2 * x * y

`y + 0` is merged with `y` by `addZero`.  By congruence `x * (y + 0)` and
`y * x` become the same e-class (`Mul` is commutative).  `double` matches
`Add(X, X)` non-linearly and produces `Mul(2, X)`.  That term is flattened
with the inner `Mul` to the multiset `{2, x, y}`.  Printing rules produce
every bracketing and order of each term as strings, and extraction keeps the
shortest one.

## Bootstrapping

`rzaddy/zaddy.py` is the precompiled bootstrap.  To rebuild it from
`zaddy.zaddy`:

    nix build .#zaddyc                 # bootstrap -> zaddyc built from zaddy.zaddy
    ./result/bin/zaddyc < zaddy.zaddy > new.py
    cmp new.py rzaddy/zaddy.py         # fixed point

`nix flake check` performs this comparison, and also runs the examples.
When changing the language, compile the new `zaddy.zaddy` with the old
compiler, then compile it again with the result, until the output stops
changing.  The runtime lives inside the `ZADDY` production as `py.*` terms.
`tools/py2zaddy.py` converts an indented Python file into that form.

## Limitations

* Rewriting modulo AC with extension can build very large e-graphs.  The
  printing rules in `arith.zaddy` materialize every sub-multiset of every sum.
  Inputs whose expressions share variables therefore interact inside one
  e-graph.  Flattening is capped at 8 arguments (`FLATCAP`), and extension is
  only attempted on terms of at most 8 arguments (`EXTCAP`).  The `-n` limit
  bounds everything else.
* Flatten evaluates every combination of alternative lists in its argument
  classes, up to 64 combinations per term (`PRODUCTCAP`).
* Field types in signatures are not checked.  Arity is checked when RPython
  translates the generated `makeCon` calls.
* Strings in Zaddy source cannot contain `'`, and their contents pass through
  one level of Python string-literal unescaping in the generated code.

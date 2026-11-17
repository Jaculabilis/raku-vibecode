This is the reference implementation of META Restricted ACE Zaddy, a language
for describing compilers: PEG grammars build terms of Zephyr-style tree
algebras, rewrite rules saturate an e-graph modulo associativity,
commutativity and equality, and the cheapest printable term is extracted.

    zaddy.zaddy      Zaddy, described in Zaddy
    rzaddy/zaddy.py  precompiled bootstrap: zaddy.zaddy compiled by itself
    bf.zaddy         Brainfuck to C, with peephole optimizations
    arith.zaddy      algebraic simplification modulo AC
    doc/zaddy.md     language reference: input format, output format, semantics
    tests/           inputs and expected outputs for `nix flake check`
    tools/           py2zaddy.py, for editing the runtime embedded in zaddy.zaddy
    vim/             syntax highlighting

Building (Linux; RPython via rpypkgs):

    nix build .#zaddyc        # also .#bfc and .#arithc
    nix flake check           # fixed point, bf and arith tests

    ./result/bin/zaddyc < bf.zaddy > bfc.py

# Smoke test for a static Raku: a cut-down Vixen expression compiler in the
# style of "An expression language for Vixen" (the grammar is copied
# verbatim from that post). It exercises what the real compiler needs:
# grammars and actions, classes and roles, Set arithmetic, %*ENV, run
# (calling into Vixen as a subprocess), spurt and chmod.

grammar Vixen {
    token id       { <[A..Za..z*]>+ <![:]> }
    token selector { <[A..Za..z*]>+ \: }
    token string   { \" <-["]>* \" }
    token param    { \: <id> }
    token ass      { \:\= }

    rule params { <param>* % <ws> }

    proto rule lit             {*}
          rule lit:sym<block>  { '[' <params> '|' <exprs> ']' }
          rule lit:sym<paren>  { '(' <call> ')' }
          rule lit:sym<string> { <string> }
          rule lit:sym<name>   { <id> }

    rule chain { <id>* % <ws> }

    proto rule unary {*}
          rule unary:sym<chain> { <lit> <chain> }

    rule keyword { <selector> <unary> }

    proto rule message {*}
          rule message:sym<key>  { <keyword>+ }

    rule messages { <message>* % ';' }

    proto rule call {*}
          rule call:sym<cascade> { <unary> <messages> }

    proto rule assign {*}
          rule assign:sym<name> { <id> <ass> <call> }
          rule assign:sym<call> { <call> }

    rule statement { <assign> '.' }

    proto rule exprs {*}
          rule exprs:sym<rv>  { <statement>* '^' <call> }
          rule exprs:sym<nil> { <statement>* <call> }

    rule TOP { '[' <params> '|' <exprs> ']' }
}

# Abstract syntax. Every expression can report the names it uses and can be
# compiled to an execline word (`literal`), emitting calls as needed.
role Expr {
    method uses(--> Set) { ... }
    method literal($compiler --> Str) { ... }
}

class VName does Expr {
    has Str $.n;
    method uses(--> Set) { set $.n }
    method literal($compiler --> Str) { '$' ~ $.n }
}

class VStr does Expr {
    has Str $.s;
    method uses(--> Set) { set() }
    method literal($compiler --> Str) { $.s }
}

class VUnary does Expr {
    has Expr $.receiver;
    has Str @.verb;
    method uses(--> Set) { $.receiver.uses }
    method literal($compiler --> Str) {
        my $answer = $.receiver.literal: $compiler;
        $answer = $compiler.callBind: $answer, $_, () for @.verb;
        $answer;
    }
}

class VKeyword {
    has Str $.selector;
    has Expr $.arg;
}

class VMessage {
    has VKeyword @.keywords;
    method uses(--> Set) { [(|)] @.keywords.map: *.arg.uses }
    method prepare($compiler) {
        @.keywords.map(*.selector).join, @.keywords.map(*.arg.literal: $compiler).List;
    }
}

class VCall does Expr {
    has Expr $.unary;
    has VMessage @.cascades;  # NB: inhabited
    method uses(--> Set) { [(|)] $.unary.uses, |@.cascades.map: *.uses }
    method literal($compiler --> Str) {
        my $receiver = $.unary.literal: $compiler;
        my $answer;
        for @.cascades {
            my ($verb, $args) = .prepare: $compiler;
            $answer = $compiler.callBind: $receiver, $verb, $args;
        }
        $answer;
    }
}

class VAssignName {
    has Str $.target;
    has Expr $.call;
    method writes(--> Set) { set $.target }
    method compile($compiler) { $compiler.define: $.target, $.call.literal: $compiler }
}

class VIgnore {
    has Expr $.call;
    method writes(--> Set) { set() }
    method compile($compiler) { $.call.literal: $compiler }
}

class VExprs {
    has @.statements;
    has Expr $.rv;
    method uses(--> Set) {
        my $names = $.rv.uses;
        for @.statements.reverse {
            $names = ($names (-) .writes) (|) .call.uses;
        }
        $names;
    }
    method compile($compiler --> Str) {
        .compile: $compiler for @.statements;
        $.rv.literal: $compiler;
    }
}

class VBlock {
    has Str @.params;
    has VExprs $.exprs;
    method uses(--> Set) { $.exprs.uses (-) @.params }
}

class VixenActions {
    method params($/)            { make $<param>.values.map(*<id>.Str).List }
    method lit:sym<block>($/)    { make VBlock.new: params => $<params>.made, exprs => $<exprs>.made }
    method lit:sym<paren>($/)    { make $<call>.made }
    method lit:sym<string>($/)   { make VStr.new: s => ~$<string> }
    method lit:sym<name>($/)     { make VName.new: n => ~$<id> }
    method chain($/)             { make $<id>.values.map(*.Str).List }
    method unary:sym<chain>($/) {
        my $receiver = $<lit>.made;
        my @verb = $<chain>.made;
        make @verb ?? VUnary.new(:$receiver, :@verb) !! $receiver;
    }
    method keyword($/)           { make VKeyword.new: selector => ~$<selector>, arg => $<unary>.made }
    method message:sym<key>($/)  { make VMessage.new: keywords => $<keyword>.values.map(*.made) }
    method messages($/)          { make $<message>.values.map(*.made).List }
    method call:sym<cascade>($/) {
        my @cascades = $<messages>.made;
        make @cascades ?? VCall.new(unary => $<unary>.made, :@cascades) !! $<unary>.made;
    }
    method assign:sym<name>($/)  { make VAssignName.new: target => ~$<id>, call => $<call>.made }
    method assign:sym<call>($/)  { make VIgnore.new: call => $<call>.made }
    method statement($/)         { make $<assign>.made }
    method exprs:sym<rv>($/) {
        make VExprs.new: statements => $<statement>.values.map(*.made), rv => $<call>.made;
    }
    method exprs:sym<nil>($/) {
        my @statements = $<statement>.values.map: *.made;
        @statements.push: VIgnore.new(call => $<call>.made);
        make VExprs.new: :@statements, rv => VName.new(n => 'Nil');
    }
    method TOP($/) { make VBlock.new: params => $<params>.made, exprs => $<exprs>.made }
}

class Compiler {
    has Int $!syms = 0;
    has Str @.lines;
    method gensym { 'gs' ~ ++$!syms }
    method push($line) { @.lines.push: $line }
    method callBind($receiver, $verb, @args --> Str) {
        my $name = self.gensym;
        self.push: "backtick -E $name \{ \$\{V\}/call: " ~ ($receiver, $verb, |@args).join(' ') ~ ' }';
        '$' ~ $name;
    }
    method define($name, $value) { self.push: "define $name $value" }
}

sub compileBlock(VBlock $block --> Str) {
    my $compiler = Compiler.new;
    my $arity = $block.params.elems + 1;
    $compiler.push: "#!/usr/bin/env -S execlineb -s$arity";
    $compiler.push: 'importas -iS V';
    $compiler.define: 'self', '$1';
    $compiler.define: $block.params[$_], '$' ~ ($_ + 2) for $block.params.keys;
    my $rv = $block.exprs.compile: $compiler;
    $compiler.push: "echo $rv";
    $compiler.lines.join("\n") ~ "\n";
}

# The Smalltix/Vixen FFI, verbatim from the post.
sub callVixen($receiver, $verb, *@args) {
    my $proc = run %*ENV<V> ~ "/call:", $receiver, $verb, |@args, :out;
    my $answer = $proc.out.slurp: :close;
    $proc.sink;
    $answer.trim;
}

my @sources =
    '[|^(self at: "parent*") exists]',
    qq:to/END/.trim;
        [:source|
        obj := (self at: "NixStore") intern: source.
        ^self at: obj name put: obj]
        END

for @sources -> $source {
    my $parsed = Vixen.parse: $source, actions => VixenActions.new;
    die "Parse error: $source" unless $parsed;
    my $block = $parsed.made;
    say "--- uses: ", $block.uses.keys.sort.join(' ');
    print compileBlock $block;
}

# Call "into Vixen": a fake V object whose call: method answers with its
# argv, plus a block written to disk with spurt/chmod and then run.
my $V = $*TMPDIR.add("vixen-test-$*PID");
mkdir $V;
spurt "$V/call:", "#!/bin/sh\necho \"answer from \$2 to \$1:\" \"\$@\"\n";
chmod 0o755, "$V/call:";
%*ENV<V> = ~$V;
say callVixen(~$V, 'clone:', 'Allocator').subst(~$V, '$V', :g);

spurt "$V/value", "#!/bin/sh\necho value of \$1\n";
chmod 0o755, "$V/value";
say run("$V/value", ~$V, :out).out.slurp(:close).trim.subst(~$V, '$V');

.unlink for dir $V;
rmdir $V;

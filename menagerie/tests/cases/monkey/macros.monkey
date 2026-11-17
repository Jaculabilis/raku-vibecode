// Quote/unquote and macros from "The Lost Chapter".
puts(quote(5));                          // expect: QUOTE(5)
puts(quote(5 + 8));                      // expect: QUOTE((5 + 8))
puts(quote(foobar + barfoo));            // expect: QUOTE((foobar + barfoo))
puts(quote(unquote(4)));                 // expect: QUOTE(4)
puts(quote(unquote(4 + 4)));             // expect: QUOTE(8)
puts(quote(8 + unquote(4 + 4)));         // expect: QUOTE((8 + 8))
puts(quote(unquote(4 + 4) + 8));         // expect: QUOTE((8 + 8))
let foobar = 8;
puts(quote(foobar));                     // expect: QUOTE(foobar)
puts(quote(unquote(foobar)));            // expect: QUOTE(8)
puts(quote(unquote(true)));              // expect: QUOTE(true)
puts(quote(unquote(true == false)));     // expect: QUOTE(false)
puts(quote(unquote(quote(4 + 4))));      // expect: QUOTE((4 + 4))
let quotedInfixExpression = quote(4 + 4);
puts(quote(unquote(4 + 4) + unquote(quotedInfixExpression))); // expect: QUOTE((8 + (4 + 4)))

let unless = macro(condition, consequence, alternative) {
  quote(if (!(unquote(condition))) { unquote(consequence); } else { unquote(alternative); });
};
unless(10 > 5, puts("not greater"), puts("greater")); // expect: greater

let reverse = macro(a, b) { quote(unquote(b) - unquote(a)); };
puts(reverse(2 + 2, 10 - 5));            // expect: 1

// Macros run at compile time, so arguments are not evaluated first.
let twice = macro(e) { quote({ "first": unquote(e), "second": unquote(e) }) };
let n = 0;
puts(twice(n + 1));                      // expect: {first: 1, second: 1}

// A macro may compute with ordinary Monkey code while expanding.
let power = macro(base, exp) {
  let build = fn(n) { if (n == 0) { quote(1) } else { quote(unquote(base) * unquote(build(n - 1))) } };
  build(3)
};
puts(power(2, 3));                       // expect: 8

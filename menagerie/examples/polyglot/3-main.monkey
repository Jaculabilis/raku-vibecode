// Monkey drives everything: it calls a Lox class, a Lox function that calls
// Tiger, and a Tiger procedure that calls back into Lox.
let tostr = fn(n) {
  let digits = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
  if (n < 10) { digits[n] } else { tostr(n / 10) + digits[n - (n / 10) * 10] }
};
puts(hello("menagerie"));          // expect: Hello, menagerie~~~
puts(Greeter("monkey"));           // expect: Greeter instance
puts(fibs(12));                    // expect: 0 1 1 2 3 5 8 13 21 34 55 89
banner("three languages, one bytecode"); // expect: three languages, one bytecode!
let twice = fn(f, x) { f(f(x)) };
puts(twice(shout, "wow"));         // expect: wow!!
puts(map([10, 20, 30], fib));      // expect: [55, 6765, 832040]

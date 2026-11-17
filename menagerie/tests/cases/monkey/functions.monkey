let identity = fn(x) { x; };
puts(identity(5));                         // expect: 5
let double = fn(x) { x * 2; };
puts(double(5));                           // expect: 10
let add = fn(x, y) { x + y; };
puts(add(5 + 5, add(5, 5)));               // expect: 20
puts(fn(x) { x; }(5));                     // expect: 5
let newAdder = fn(x) { fn(y) { x + y } };
let addTwo = newAdder(2);
puts(addTwo(2));                           // expect: 4
let noReturn = fn() { };
puts(noReturn());                          // expect: null
let fib = fn(x) { if (x == 0) { 0 } else { if (x == 1) { 1 } else { fib(x - 1) + fib(x - 2) } } };
puts(fib(15));                             // expect: 610
let countDown = fn(x) { if (x == 0) { return 0; } else { countDown(x - 1); } };
let wrapper = fn() { countDown(1); };
puts(wrapper());                           // expect: 0
// Deep (non-tail) recursion runs on the VM's heap-allocated frame stack.
let sum = fn(n) { if (n == 0) { 0 } else { n + sum(n - 1) } };
puts(sum(100000));                         // expect: 5000050000
puts(fn(a, b) { a + b });                  // expect: fn(a, b) {
                                           // expect: (a + b)
                                           // expect: }
puts(len);                                 // expect: builtin function
let map = fn(arr, f) {
  let iter = fn(arr, acc) { if (len(arr) == 0) { acc } else { iter(rest(arr), push(acc, f(first(arr)))) } };
  iter(arr, [])
};
let reduce = fn(arr, initial, f) {
  let iter = fn(arr, result) { if (len(arr) == 0) { result } else { iter(rest(arr), f(result, first(arr))) } };
  iter(arr, initial)
};
puts(map([1, 2, 3, 4], fn(x) { x * x }));  // expect: [1, 4, 9, 16]
puts(reduce([1, 2, 3, 4, 5], 0, fn(a, b) { a + b })); // expect: 15

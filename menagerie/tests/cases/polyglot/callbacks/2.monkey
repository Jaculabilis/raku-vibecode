let callTimes = fn(f, n) { if (n == 0) { f() } else { f(); callTimes(f, n - 1) } };
puts(callTimes(c, 4));       // expect: 5
let adder = fn(k) { fn(x) { x + k } };

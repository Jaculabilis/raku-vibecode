// Monkey's reference semantics (the tree-walking evaluator): one
// environment per call, `let` rebinds in it, closures share it, and unbound
// names are looked up in the enclosing environment.
let x = 1;
let f = fn() { x };
let x = 2;
puts(f());                       // expect: 2
let g = fn() { let h = fn() { y }; let y = 5; h() };
puts(g());                       // expect: 5
let k = fn() { puts(x); let x = 10; puts(x); };
k();                             // expect: 2
                                 // expect: 10
let blocks = fn(c) { if (c) { let z = "inner"; } z };
puts(blocks(true));              // expect: inner
let z = "global";
puts(blocks(false));             // expect: global
let counter = fn() {
  let n = 0;
  let get = fn() { n };
  let n = n + 1;
  let n = n + 1;
  get()
};
puts(counter());                 // expect: 2
let len = fn(x) { "shadowed" };
puts(len([1, 2]));               // expect: shadowed

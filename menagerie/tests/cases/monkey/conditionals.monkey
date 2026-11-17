puts(if (true) { 10 });               // expect: 10
puts(if (false) { 10 });              // expect: null
puts(if (1) { 10 });                  // expect: 10
puts(if (0) { 10 } else { 20 });      // expect: 10
puts(if (1 > 2) { 10 } else { 20 });  // expect: 20
puts(if (if (false) { 10 }) { 10 } else { 20 }); // expect: 20
puts(if (true) { let x = 1; });       // expect: null
let r = fn() { if (10 > 1) { if (10 > 1) { return 10; } return 1; } };
puts(r());                            // expect: 10
let early = fn(x) { if (x > 5) { return "big"; } "small" };
puts(early(10), early(1));            // expect: big
                                      // expect: small

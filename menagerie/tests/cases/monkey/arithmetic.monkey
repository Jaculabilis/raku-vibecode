// Expressions from the book's evaluator tests.
puts(5 + 5 + 5 + 5 - 10);        // expect: 10
puts(2 * 2 * 2 * 2 * 2);         // expect: 32
puts(-50 + 100 + -50);           // expect: 0
puts(5 * 2 + 10);                // expect: 20
puts(5 + 2 * 10);                // expect: 25
puts(20 + 2 * -10);              // expect: 0
puts(50 / 2 * 2 + 10);           // expect: 60
puts(2 * (5 + 10));              // expect: 30
puts(3 * 3 * 3 + 10);            // expect: 37
puts((5 + 10 * 2 + 15 / 3) * 2 + -10); // expect: 50
puts(-7 / 2);                    // expect: -3
puts(9223372036854775807 + 1);   // expect: -9223372036854775808
puts(1 < 2, 1 > 2, 1 == 1, 1 != 1); // expect: true
                                 // expect: false
                                 // expect: true
                                 // expect: false
puts(true == true, true != false, (1 < 2) == true, (1 > 2) == true); // expect: true
                                 // expect: true
                                 // expect: true
                                 // expect: false
puts(!true, !5, !!true, !!5, !0); // expect: false
                                  // expect: false
                                  // expect: true
                                  // expect: true
                                  // expect: false
puts(1 == true, "a" == "a", [1] == [1]); // expect: false
                                         // expect: true
                                         // expect: false

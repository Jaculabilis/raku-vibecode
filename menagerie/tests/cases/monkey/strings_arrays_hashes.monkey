puts("Hello" + " " + "World!");     // expect: Hello World!
puts(len(""), len("four"), len("hello world")); // expect: 0
                                    // expect: 4
                                    // expect: 11
puts("tab:\tquote:\" newline:\nend"); // expect: tab:	quote:" newline:
                                    // expect: end
let a = [1, 2 * 2, 3 + 3];
puts(a, a[0], a[1 + 1]);            // expect: [1, 4, 6]
                                    // expect: 1
                                    // expect: 6
puts([1, 2, 3][3], [1, 2, 3][-1]);  // expect: null
                                    // expect: null
let myArray = [1, 2, 3]; let i = myArray[0];
puts(myArray[i]);                   // expect: 2
puts(len(a), first(a), last(a), rest(a), push(a, 7), a); // expect: 3
                                    // expect: 1
                                    // expect: 6
                                    // expect: [4, 6]
                                    // expect: [1, 4, 6, 7]
                                    // expect: [1, 4, 6]
puts(first([]), last([]), rest([])); // expect: null
                                    // expect: null
                                    // expect: null
let two = "two";
let h = {"one": 10 - 9, two: 1 + 1, "thr" + "ee": 6 / 2, 4: 4, true: 5, false: 6};
puts(h["one"], h["two"], h["three"], h[4], h[true], h[false], h["nope"]);
// expect: 1
// expect: 2
// expect: 3
// expect: 4
// expect: 5
// expect: 6
// expect: null
puts({"a": [1, "x"], 2: {}});       // expect: {a: [1, x], 2: {}}
puts({"k": 1, "k": 2}["k"]);        // expect: 2

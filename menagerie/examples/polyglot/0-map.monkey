let map = fn(arr, f) {
  let iter = fn(arr, acc) {
    if (len(arr) == 0) { acc } else { iter(rest(arr), push(acc, f(first(arr)))) }
  };
  iter(arr, [])
};

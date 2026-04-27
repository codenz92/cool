use std::collections::HashMap;

fn kernel(n: usize) -> i64 {
    let mut items = Vec::with_capacity(n);
    let mut lookup = HashMap::with_capacity(n);
    let mut i = 0usize;
    while i < n {
        let value = ((i as i64) * 7) % 100_003;
        items.push(value);
        lookup.insert(i as i64, value + 1);
        i += 1;
    }

    let mut acc = 0i64;
    i = 0;
    while i < n {
        acc += items[i];
        acc += lookup[&(i as i64)];
        i += 1;
    }
    acc
}

fn main() {
    let n = std::hint::black_box(80_000usize);
    println!("{}", kernel(n));
}

fn kernel(n: i64) -> i64 {
    let mut acc = 0i64;
    let mut i = 0i64;
    while i < n {
        acc += (i * 3) ^ (i >> 1);
        acc += i % 97;
        i += 1;
    }
    acc
}

fn main() {
    let n = std::hint::black_box(4_000_000i64);
    println!("{}", kernel(n));
}

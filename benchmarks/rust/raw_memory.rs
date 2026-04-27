fn kernel(n: usize) -> i64 {
    let mut buf = vec![0i64; n];
    let ptr = buf.as_mut_ptr();
    let mut i = 0usize;
    while i < n {
        unsafe {
            *ptr.add(i) = (i as i64) * 3;
        }
        i += 1;
    }

    let mut acc = 0i64;
    i = 0;
    while i < n {
        unsafe {
            acc += *ptr.add(i);
        }
        i += 1;
    }
    acc
}

fn main() {
    let n = std::hint::black_box(600_000usize);
    println!("{}", kernel(n));
}

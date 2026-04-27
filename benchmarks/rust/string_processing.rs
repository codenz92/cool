const BASE: &str =
    "abc lorem ipsum xyz 012345 abc lorem ipsum xyz 012345 abc lorem ipsum xyz 012345 \
     abc lorem ipsum xyz 012345 abc lorem ipsum xyz 012345 abc lorem ipsum xyz 012345 \
     abc lorem ipsum xyz 012345 abc lorem ipsum xyz 012345 abc lorem ipsum xyz 012345 \
     abc lorem ipsum xyz 012345 abc lorem ipsum xyz 012345 abc lorem ipsum xyz 012345 ";

fn kernel(n: usize) -> i64 {
    let mut text = BASE.to_string();
    let mut acc = 0i64;
    let mut i = 0usize;
    while i < n {
        acc += text.matches("abc").count() as i64;
        acc += text.find("xyz").expect("missing xyz") as i64;
        text = text.replace("abc", "abd");
        text = text.replace("abd", "abc");
        i += 1;
    }
    acc + text.len() as i64
}

fn main() {
    let n = std::hint::black_box(15_000usize);
    println!("{}", kernel(n));
}

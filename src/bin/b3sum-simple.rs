use core::fmt::Write;

const INPUT: &[u8] = include_bytes!("../../benches/element-web-v1.10.10-vendors~init.js");

fn main() {
    let mut hasher = ::blake3_simple::Hasher::default();
    hasher.update(INPUT);
    let hash = hasher.finalize();
    let hash = hash.as_bytes();
    let mut s = String::with_capacity(2 * hash.len());
    for byte in hash {
        write!(s, "{:02x}", byte).expect("can't write hex byte to hex buffer")
    }
    println!("{}", s);
}

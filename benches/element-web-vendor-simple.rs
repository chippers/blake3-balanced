use criterion::{black_box, criterion_group, criterion_main, Criterion};

const INPUT: &[u8] = include_bytes!("element-web-v1.10.10-vendors~init.js");

pub fn bench_element_web_vendor_simple(c: &mut Criterion) {
    c.bench_function("simple", |b| {
        b.iter(|| {
            let mut hasher = ::blake3_simple::Hasher::new();
            hasher.update(black_box(INPUT));
            hasher.finalize()
        })
    });
}

criterion_group!(benches, bench_element_web_vendor_simple);
criterion_main!(benches);

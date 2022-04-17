use std::time::Duration;
use criterion::{black_box, criterion_group, criterion_main, Criterion};

const INPUT: &[u8] = include_bytes!("element-web-v1.10.10-vendors~init.js");

fn blake3(data: &[u8]) -> ::blake3::Hash {
    ::blake3::hash(data)
}

fn blake3_rayon(data: &[u8]) -> ::blake3::Hash {
    let mut hasher = ::blake3::Hasher::default();
    hasher.update_rayon(data);
    hasher.finalize()
}

fn blake3_reference(data: &[u8]) -> [u8; 32] {
    let mut hasher = ::blake3_reference::Hasher::new();
    hasher.update(data);
    let mut out = [0; 32];
    hasher.finalize(&mut out);
    out
}

fn blake3_simple(data: &[u8]) -> [u8; 32] {
    let mut hasher = ::blake3_simple::Hasher::new();
    hasher.update(data);
    let mut out = [0; 32];
    hasher.finalize(&mut out);
    out
}

pub fn bench_element_web_vendor(c: &mut Criterion) {
    let mut group = c.benchmark_group("element-web-vendor");
    group.measurement_time(Duration::from_secs(10));
    group.bench_function("blake3", |b| b.iter(|| blake3(black_box(INPUT))));
    group.bench_function("blake3-rayon", |b| {
        b.iter(|| blake3_rayon(black_box(INPUT)))
    });
    group.bench_function("blake3-simple", |b| {
        b.iter(|| blake3_simple(black_box(INPUT)))
    });
    group.bench_function("reference", |b| {
        b.iter(|| blake3_reference(black_box(INPUT)))
    });
    group.finish();
}

criterion_group!(benches, bench_element_web_vendor);
criterion_main!(benches);

# blake3-simple

A simple implementation of BLAKE3 based on the BLAKE3 Rust reference.

## why?

This project was inspired from using BLAKE3 as a file hasing algorithm during
the codegen stage of [Tauri](https://github.com/tauri-apps/tauri/). Since the
codegen code runs during compilation, the runtime of the build script also
includes the compile time. The [`blake3`]
crate includes optimized implementations for SSE2, SSE4.1, AVX2, AVX-512 using
C. When compiling multiple C-compiling dependencies with cargo, the overall
compile time of each C-compiling build script inflates significantly while
competing for resources. [See this GitHub thread] detailing some experiences
with that.

Using the BLAKE3 rust reference has significantly reduced runtime speed
compared to the [`blake3`] crate but it compiles almost instantly. When using
it in situations where run time includes compilation time, this can lead to
overall speed gains when using on a limited amount of input.

As a direct example comparing these compile/run times, here are some timings
collected on a ThinkPad T410 Intel i5-520M[^1]. The compile time was measured
using the `--timings` flag on Cargo and the run time with [hyperfine]. The file
hashed was a 6.8MiB JavaScript file.

| crate | features | compile time | run time |
| --- | --- | --- | --- |
| `blake3` | `rayon` | 19.2s | 8.7ms |
| `blake3` | | 8.5s | 14.9ms |
| `blake3_reference` | | 1.5s | 37.9ms |

[^1]: ThinkPad T410 Intel i5-520M, 8 GiB RAM, 120GB SSD.

While the reference has a 4x slowdown compared to the optimized multi-threaded
runtime of [`blake3`], the overall time when used in codegen code is
significantly faster due to the very fast compile time.

In summary, the answer to "why?" is that this is an appealing alternative to
vendoring the BLAKE3 rust references in multiple projects where these
characteristics are desired.

## benchmarks

There are benchmarks in an attempt to keep track of the run time differences
between [`blake3`], the BLAKE3 Rust reference, and `blake3-simple` in addition
to measure and improve performance of `blake3-simple`. The input file that is
used is the JavaScript vendor file from v1.10.10 of [element-web]. To run the
benchmarks, use `cargo-criterion`.

1. install: `cargo install cargo-criterion`
2. run: `cargo criterion`

[`blake3`]: https://crates.io/crates/blake3
[See this GitHub thread]: https://github.com/BLAKE3-team/BLAKE3/pull/228
[hyperfine]: https://github.com/sharkdp/hyperfine
[element-web]: https://github.com/vector-im/element-web

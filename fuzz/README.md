# blake3-simple-fuzz

Fuzzing output of [`blake3`], the [BLAKE3 rust reference], and
[`blake3-simple`] using [afl.rs]. This fuzzing cargo project has been set up
using instructions from the [afl.rs section of the Rust Fuzz Book]. The parity
fuzzing target not only fuzzes the output of the mentioned 3 BLAKE3
implementations, but asserts that their byte output matches the rust reference.

## running

Make sure that `cargo-afl` is installed by running `cargo install afl`.

We use the test cases provided by [AFL], so make sure that the submodule is
checked out. The following commands can then be ran inside this directory.

1. build the fuzzing binary: `cargo afl build`
2. run the binary against the test cases: `cargo afl fuzz -i AFL/testcases -o out target/debug/blake3-simple-fuzz`


[`blake3`]: https://github.com/BLAKE3-team/BLAKE3
[BLAKE3 rust reference]: https://github.com/BLAKE3-team/BLAKE3/tree/master/reference_impl
[`blake3-simple`]: ..
[afl.rs]: https://github.com/rust-fuzz/afl.rs
[afl.rs section of the Rust Fuzz Book]: https://rust-fuzz.github.io/book/afl.html
[AFL]: https://github.com/google/AFL

fn main() {
    ::afl::fuzz!(|data: &[u8]| {
        let standard = {
            let mut hasher = ::blake3::Hasher::default();
            hasher.update_rayon(data);
            *hasher.finalize().as_bytes()
        };

        let reference = {
            let mut hasher = ::blake3_reference::Hasher::new();
            hasher.update(data);
            let mut out = [0; 32];
            hasher.finalize(&mut out);
            out
        };

        let simple = {
            let mut hasher = ::blake3_simple::Hasher::new();
            hasher.update(data);
            let mut out = [0; 32];
            hasher.finalize(&mut out);
            out
        };

        for ((&reference, standard), simple) in reference.iter().zip(standard).zip(simple) {
            assert_eq!(reference, standard);
            assert_eq!(reference, simple);
        }
    });
}

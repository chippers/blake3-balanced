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

        let balanced = {
            let mut hasher = ::blake3_balanced::Hasher::new();
            hasher.update(data);
            *hasher.finalize().as_bytes()
        };

        for ((&reference, standard), balanced) in reference.iter().zip(standard).zip(balanced) {
            assert_eq!(reference, standard);
            assert_eq!(reference, balanced);
        }
    });
}

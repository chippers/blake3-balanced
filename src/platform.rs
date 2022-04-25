use core::convert::TryInto;

#[inline(always)]
fn word_to_bytes(bytes: &mut [u8], word: u32) {
    let [byte0, byte1, byte2, byte3] = word.to_le_bytes();
    bytes[0] = byte0;
    bytes[1] = byte1;
    bytes[2] = byte2;
    bytes[3] = byte3;
}

#[inline(always)]
pub(crate) fn le_bytes_from_words_32(words: &[u32; 8]) -> [u8; 32] {
    let mut out = [0; 32];
    for (bytes, &word) in out.chunks_exact_mut(4).zip(words) {
        word_to_bytes(bytes, word);
    }
    out
}

#[inline(always)]
pub(crate) fn words_from_le_bytes_32(bytes: &[u8; 32]) -> [u32; 8] {
    let mut out = [0; 8];
    for (bytes, word) in bytes.chunks_exact(4).zip(&mut out) {
        *word = u32::from_le_bytes(
            bytes
                .try_into()
                .expect("chunks_exact(4) returned a chunk not size 4"),
        );
    }
    out
}

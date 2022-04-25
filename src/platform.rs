use core::convert::TryInto;

macro_rules! array_ref {
    ($arr:expr, $idx:expr, $len:expr) => {{
        {
            #[inline(always)]
            fn as_array<T>(slice: &[T]) -> &[T; $len] {
                use ::core::convert::TryInto;
                slice.try_into().expect("slice into array")
            }
            as_array(&$arr[$idx..($idx + $len)])
        }
    }};
}
pub(crate) use array_ref;

macro_rules! array_ref_mut {
    ($arr:expr, $idx:expr, $len:expr) => {{
        {
            #[inline(always)]
            fn as_array_mut<T>(slice: &mut [T]) -> &mut [T; $len] {
                use ::core::convert::TryInto;
                slice.try_into().expect("slice into array")
            }
            as_array_mut(&mut $arr[$idx..($idx + $len)])
        }
    }};
}
pub(crate) use array_ref_mut;

#[inline(always)]
pub(crate) fn le_bytes_from_words_32(words: &[u32; 8]) -> [u8; 32] {
    let mut out = [0; 32];
    for (bytes, &word) in out.chunks_exact_mut(4).zip(words) {
        let [byte0, byte1, byte2, byte3] = word.to_le_bytes();
        bytes[0] = byte0;
        bytes[1] = byte1;
        bytes[2] = byte2;
        bytes[3] = byte3;
    }
    out
}

#[inline(always)]
pub(crate) fn words_from_le_bytes_32(bytes: &[u8; 32]) -> [u32; 8] {
    let mut out = [0; 8];
    for (word, bytes) in out.iter_mut().zip(bytes.chunks_exact(4)) {
        *word = u32::from_le_bytes(
            bytes
                .try_into()
                .expect("chunks_exact(4) returned a chunk not size 4"),
        );
    }
    out
}

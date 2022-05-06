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

macro_rules! word_to_bytes {
    ($words:expr, $out:expr, $idx:expr) => {
        let [byte0, byte1, byte2, byte3] = $words[$idx].to_le_bytes();
        $out[$idx * 4] = byte0;
        $out[($idx * 4) + 1] = byte1;
        $out[($idx * 4) + 2] = byte2;
        $out[($idx * 4) + 3] = byte3;
    };
}

#[inline(always)]
pub(crate) fn le_bytes_from_words_32(words: &[u32; 8]) -> [u8; 32] {
    let mut out = [0; 32];

    word_to_bytes!(words, out, 0);
    word_to_bytes!(words, out, 1);
    word_to_bytes!(words, out, 2);
    word_to_bytes!(words, out, 3);
    word_to_bytes!(words, out, 4);
    word_to_bytes!(words, out, 5);
    word_to_bytes!(words, out, 6);
    word_to_bytes!(words, out, 7);

    out
}

macro_rules! bytes_to_word {
    ($bytes:expr, $out:expr, $idx:expr) => {
        $out[$idx] = u32::from_le_bytes([
            $bytes[$idx * 4],
            $bytes[($idx * 4) + 1],
            $bytes[($idx * 4) + 2],
            $bytes[($idx * 4) + 3],
        ])
    };
}

#[inline(always)]
pub(crate) fn words_from_le_bytes_32(bytes: &[u8; 32]) -> [u32; 8] {
    let mut out = [0; 8];

    bytes_to_word!(bytes, out, 0);
    bytes_to_word!(bytes, out, 1);
    bytes_to_word!(bytes, out, 2);
    bytes_to_word!(bytes, out, 3);
    bytes_to_word!(bytes, out, 4);
    bytes_to_word!(bytes, out, 5);
    bytes_to_word!(bytes, out, 6);
    bytes_to_word!(bytes, out, 7);

    out
}

#[cfg(not(complex))]
#[inline(always)]
pub(crate) fn words_from_le_bytes_64(bytes: &[u8; 64]) -> [u32; 16] {
    let mut out = [0; 16];

    bytes_to_word!(bytes, out, 0);
    bytes_to_word!(bytes, out, 1);
    bytes_to_word!(bytes, out, 2);
    bytes_to_word!(bytes, out, 3);
    bytes_to_word!(bytes, out, 4);
    bytes_to_word!(bytes, out, 5);
    bytes_to_word!(bytes, out, 6);
    bytes_to_word!(bytes, out, 7);
    bytes_to_word!(bytes, out, 8);
    bytes_to_word!(bytes, out, 9);
    bytes_to_word!(bytes, out, 10);
    bytes_to_word!(bytes, out, 11);
    bytes_to_word!(bytes, out, 12);
    bytes_to_word!(bytes, out, 13);
    bytes_to_word!(bytes, out, 14);
    bytes_to_word!(bytes, out, 15);

    out
}

use core::{
    convert::TryInto,
    ops::{Add, AddAssign, BitAnd, BitOr, BitXorAssign, Not},
};

use crate::{counter_high, counter_low, CVBytes, CVWords, BLOCK_LEN, IV, OUT_LEN};
use arrayref::{array_mut_ref, array_ref};

//#[derive(Clone, Copy)]
struct State {
    row0: Row,
    row1: Row,
    row2: Row,
    row3: Row,
}

impl<T: Into<Row>> From<(T, T, T, T)> for State {
    fn from(rows: (T, T, T, T)) -> Self {
        Self {
            row0: rows.0.into(),
            row1: rows.1.into(),
            row2: rows.2.into(),
            row3: rows.3.into(),
        }
    }
}

impl State {
    fn take(&mut self) -> Self {
        ::core::mem::replace(
            self,
            Self::from(([0u32; 4], [0u32; 4], [0u32; 4], [0u32; 4])),
        )
    }
}

macro_rules! ar {
    ($arr:expr, $idx:expr, $len:expr) => {{
        {
            fn as_array<T>(slice: &[T]) -> [T; $len]
            where
                T: Copy,
            {
                slice.try_into().expect("failed to make slice into array")
            }
            as_array(&$arr[$idx..($idx + $len)])
        }
    }};
}

#[derive(Clone, Copy)]
struct Row([u32; 4]);

impl From<[u32; 4]> for Row {
    fn from(row: [u32; 4]) -> Self {
        Self(row)
    }
}

impl From<[u8; 16]> for Row {
    fn from(bytes: [u8; 16]) -> Self {
        Self([
            u32::from_le_bytes(ar!(bytes, 0, 4)),
            u32::from_le_bytes(ar!(bytes, 4, 4)),
            u32::from_le_bytes(ar!(bytes, 8, 4)),
            u32::from_le_bytes(ar!(bytes, 12, 4)),
        ])
    }
}

impl Row {
    fn rot(&mut self, n: u32) {
        for word in &mut self.0 {
            *word = word.rotate_right(n);
        }
    }

    /// Equivalent to `_mm_shuffle_epi32(src, _MM_SHUFFLE!(z, y, x, w))`

    fn shuffle(row: Self, z: usize, y: usize, x: usize, w: usize) -> Self {
        let row = row.0;
        Self([row[w], row[x], row[y], row[z]])
    }

    /// Equivalent to `shuffle2!(src1, src2, _MM_SHUFFLE!(z, y, x, w))`
    ///
    /// ```ignore
    /// macro_rules! shuffle2 {
    ///   ($a:expr, $b:expr, $c:expr) => {
    ///     _mm_castps_si128(_mm_shuffle_ps(
    ///       _mm_castsi128_ps($a),
    ///       _mm_castsi128_ps($b),
    ///       $c,
    ///     ))
    ///   };
    /// }
    /// ```

    fn shuffle2(lhs: Self, rhs: Self, z: usize, y: usize, x: usize, w: usize) -> Self {
        let (lhs, rhs) = (lhs.0, rhs.0);
        Self([lhs[w], lhs[x], rhs[y], rhs[z]])
    }
}

impl Add<Self> for Row {
    type Output = Row;

    fn add(mut self, rhs: Self) -> Self::Output {
        for (lhs, rhs) in self.0.iter_mut().zip(rhs.0) {
            *lhs = lhs.wrapping_add(rhs)
        }
        self
    }
}

impl AddAssign<Self> for Row {
    fn add_assign(&mut self, rhs: Self) {
        for (lhs, rhs) in self.0.iter_mut().zip(rhs.0) {
            *lhs = lhs.wrapping_add(rhs)
        }
    }
}

impl BitXorAssign<Self> for Row {
    fn bitxor_assign(&mut self, rhs: Self) {
        for (lhs, rhs) in self.0.iter_mut().zip(rhs.0) {
            *lhs ^= rhs
        }
    }
}

impl BitAnd<Self> for Row {
    type Output = Self;

    fn bitand(mut self, rhs: Self) -> Self::Output {
        for (lhs, rhs) in self.0.iter_mut().zip(rhs.0) {
            *lhs &= rhs
        }
        self
    }
}

impl BitOr<Self> for Row {
    type Output = Self;

    fn bitor(mut self, rhs: Self) -> Self::Output {
        for (lhs, rhs) in self.0.iter_mut().zip(rhs.0) {
            *lhs |= rhs;
        }
        self
    }
}

impl Not for Row {
    type Output = Self;

    fn not(mut self) -> Self::Output {
        for word in self.0.iter_mut() {
            *word = !&*word
        }
        self
    }
}

fn g1(s: &mut State, m: Row) {
    s.row0 = (s.row0 + m) + s.row1;
    s.row3 ^= s.row0;
    s.row3.rot(16);
    s.row2 += s.row3;
    s.row1 ^= s.row2;
    s.row1.rot(12)
}

fn g2(s: &mut State, m: Row) {
    s.row0 = (s.row0 + m) + s.row1;
    s.row3 ^= s.row0;
    s.row3.rot(8);
    s.row2 += s.row3;
    s.row1 ^= s.row2;
    s.row1.rot(7)
}

// Note the optimization here of leaving row1 as the unrotated row, rather than
// row0. All the message loads below are adjusted to compensate for this. See
// discussion at https://github.com/sneves/blake2-avx2/pull/4

fn diagonalize(s: &mut State) {
    s.row0 = Row::shuffle(s.row0, 2, 1, 0, 3);
    s.row3 = Row::shuffle(s.row3, 1, 0, 3, 2);
    s.row2 = Row::shuffle(s.row2, 0, 3, 2, 1);
}

fn undiagonalize(s: &mut State) {
    s.row0 = Row::shuffle(s.row0, 0, 3, 2, 1);
    s.row3 = Row::shuffle(s.row3, 1, 0, 3, 2);
    s.row2 = Row::shuffle(s.row2, 2, 1, 0, 3);
}

fn blend_epi16(lhs: Row, rhs: Row, imm8: i32) -> Row {
    let bits = Row([0x0001_0002, 0x0004_0008, 0x0010_0020, 0x0040_0080]);
    let mut mask = {
        let bytes = (imm8 as i16).to_le_bytes();
        let imm8_32 = u32::from_le_bytes([bytes[0], bytes[1], bytes[0], bytes[1]]);
        Row([imm8_32, imm8_32, imm8_32, imm8_32])
    };

    mask = mask & bits;

    // _mm_cmpeq_epi16
    mask = {
        let mut out = [0u8; 16];
        for idx in 0..mask.0.len() {
            let [m0, m1, m2, m3] = mask.0[idx].to_le_bytes();
            let [b0, b1, b2, b3] = bits.0[idx].to_le_bytes();

            let first = cmp_u16(m0, m1, b0, b1);
            out[idx * 4] = first;
            out[(idx * 4) + 1] = first;

            let second = cmp_u16(m2, m3, b2, b3);
            out[(idx * 4) + 2] = second;
            out[(idx * 4) + 3] = second;
        }
        Row::from(out)
    };

    (mask & rhs) | (!mask & lhs)
}

fn cmp_u16(l0: u8, l1: u8, r0: u8, r1: u8) -> u8 {
    if l0 == r0 && l1 == r1 {
        0xFF
    } else {
        0x00
    }
}

fn unpacklo_epi64(lhs: Row, rhs: Row) -> Row {
    Row([lhs.0[0], lhs.0[1], rhs.0[0], rhs.0[1]])
}

fn unpacklo_epi32(lhs: Row, rhs: Row) -> Row {
    Row([lhs.0[0], rhs.0[0], lhs.0[1], rhs.0[1]])
}

fn unpackhi_epi32(lhs: Row, rhs: Row) -> Row {
    Row([lhs.0[2], rhs.0[2], lhs.0[3], rhs.0[3]])
}

macro_rules! round2plus {
    ($state:expr, $m:expr, $t:expr, $tt:expr) => {
        $t.row0 = Row::shuffle2($m.row0, $m.row1, 3, 1, 1, 2);
        $t.row0 = Row::shuffle($t.row0, 0, 3, 2, 1);
        g1(&mut $state, $t.row0);
        $t.row1 = Row::shuffle2($m.row2, $m.row3, 3, 3, 2, 2);
        $tt = Row::shuffle($m.row0, 0, 0, 3, 3);
        $t.row1 = blend_epi16($tt, $t.row1, 0xCC);
        g2(&mut $state, $t.row1);
        diagonalize(&mut $state);
        $t.row2 = unpacklo_epi64($m.row3, $m.row1);
        $tt = blend_epi16($t.row2, $m.row2, 0xC0);
        $t.row2 = Row::shuffle($tt, 1, 3, 2, 0);
        g1(&mut $state, $t.row2);
        $t.row3 = unpackhi_epi32($m.row1, $m.row3);
        $tt = unpacklo_epi32($m.row2, $t.row3);
        $t.row3 = Row::shuffle($tt, 0, 1, 3, 2);
        g2(&mut $state, $t.row3);
        undiagonalize(&mut $state);
    };
}

fn compress_pre(
    cv: &CVWords,
    block: &[u8; BLOCK_LEN],
    block_len: u8,
    counter: u64,
    flags: u8,
) -> [u32; 16] {
    let mut state = State::from((
        [cv[0], cv[1], cv[2], cv[3]],
        [cv[4], cv[5], cv[6], cv[7]],
        [IV[0], IV[1], IV[2], IV[3]],
        [
            counter_low(counter),
            counter_high(counter),
            block_len as u32,
            flags as u32,
        ],
    ));

    let mut m = State::from((
        ar!(block, 0, 16),
        ar!(block, 16, 16),
        ar!(block, 32, 16),
        ar!(block, 48, 16),
    ));

    let mut t = State::from(([0u32; 4], [0u32; 4], [0u32; 4], [0u32; 4]));

    // we only use it from the macro, but want it existing for all of them
    #[allow(clippy::needless_late_init)]
    let mut tt;

    // Round 1. The first round permutes the message words from the original
    // input order, into the groups that get mixed in parallel.
    t.row0 = Row::shuffle2(m.row0, m.row1, 2, 0, 2, 0);
    g1(&mut state, t.row0);
    t.row1 = Row::shuffle2(m.row0, m.row1, 3, 1, 3, 1);
    g2(&mut state, t.row1);
    diagonalize(&mut state);
    t.row2 = Row::shuffle2(m.row2, m.row3, 2, 0, 2, 0);
    t.row2 = Row::shuffle(t.row2, 2, 1, 0, 3);
    g1(&mut state, t.row2);
    t.row3 = Row::shuffle2(m.row2, m.row3, 3, 1, 3, 1);
    t.row3 = Row::shuffle(t.row3, 2, 1, 0, 3);
    g2(&mut state, t.row3);
    undiagonalize(&mut state);
    m = t.take();

    // Round 2. This round and all following rounds apply a fixed permutation
    // to the message words from the round before.
    round2plus!(state, m, t, tt);
    m = t.take();

    // round 3
    round2plus!(state, m, t, tt);
    m = t.take();

    // round 4
    round2plus!(state, m, t, tt);
    m = t.take();

    // round 5
    round2plus!(state, m, t, tt);
    m = t.take();

    // round 6
    round2plus!(state, m, t, tt);
    m = t.take();

    // round 7
    round2plus!(state, m, t, tt);

    [
        state.row0.0[0],
        state.row0.0[1],
        state.row0.0[2],
        state.row0.0[3],
        state.row1.0[0],
        state.row1.0[1],
        state.row1.0[2],
        state.row1.0[3],
        state.row2.0[0],
        state.row2.0[1],
        state.row2.0[2],
        state.row2.0[3],
        state.row3.0[0],
        state.row3.0[1],
        state.row3.0[2],
        state.row3.0[3],
    ]
}

pub fn compress_in_place(
    cv: &mut CVWords,
    block: &[u8; BLOCK_LEN],
    block_len: u8,
    counter: u64,
    flags: u8,
) {
    let state = compress_pre(cv, block, block_len, counter, flags);

    cv[0] = state[0] ^ state[8];
    cv[1] = state[1] ^ state[9];
    cv[2] = state[2] ^ state[10];
    cv[3] = state[3] ^ state[11];
    cv[4] = state[4] ^ state[12];
    cv[5] = state[5] ^ state[13];
    cv[6] = state[6] ^ state[14];
    cv[7] = state[7] ^ state[15];
}

pub fn hash1<const N: usize>(
    input: &[u8; N],
    key: &CVWords,
    counter: u64,
    flags: u8,
    flags_start: u8,
    flags_end: u8,
    out: &mut CVBytes,
) {
    debug_assert_eq!(N % BLOCK_LEN, 0, "uneven blocks");
    let mut cv = *key;
    let mut block_flags = flags | flags_start;
    let mut slice = &input[..];
    while slice.len() >= BLOCK_LEN {
        if slice.len() == BLOCK_LEN {
            block_flags |= flags_end;
        }
        compress_in_place(
            &mut cv,
            array_ref!(slice, 0, BLOCK_LEN),
            BLOCK_LEN as u8,
            counter,
            block_flags,
        );
        block_flags = flags;
        slice = &slice[BLOCK_LEN..];
    }
    *out = crate::platform::le_bytes_from_words_32(&cv);
}

pub fn hash_many<const N: usize>(
    inputs: &[&[u8; N]],
    key: &CVWords,
    counter: u64,
    flags: u8,
    flags_start: u8,
    flags_end: u8,
    out: &mut [u8],
) {
    debug_assert!(out.len() >= inputs.len() * OUT_LEN, "out too short");
    for (&input, output) in inputs.iter().zip(out.chunks_exact_mut(OUT_LEN)) {
        hash1(
            input,
            key,
            counter,
            flags,
            flags_start,
            flags_end,
            array_mut_ref!(output, 0, OUT_LEN),
        );
    }
}

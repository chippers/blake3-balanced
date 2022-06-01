use crate::platform::{array_ref, array_ref_mut};
use crate::{counter_high, counter_low, CVBytes, CVWords, BLOCK_LEN, IV, OUT_LEN};
use core::{
    iter::{Map, Zip},
    ops::{Add, AddAssign, BitAnd, BitOr, BitXorAssign, Not},
    slice::{Iter, IterMut},
};

type IterOps<'a, 'b, 'c> = Map<
    Zip<IterMut<'a, u32>, Zip<Iter<'b, u32>, Iter<'c, u32>>>,
    for<'r> fn((&'r mut u32, (&u32, &u32))) -> (&'r mut u32, u32, u32),
>;

struct Row([u32; 4]);

impl From<[u32; 4]> for Row {
    #[inline(always)]
    fn from(row: [u32; 4]) -> Self {
        Self(row)
    }
}

impl From<&[u8; 16]> for Row {
    #[inline(always)]
    fn from(bytes: &[u8; 16]) -> Self {
        Self([
            u32::from_le_bytes(*array_ref!(bytes, 0, 4)),
            u32::from_le_bytes(*array_ref!(bytes, 4, 4)),
            u32::from_le_bytes(*array_ref!(bytes, 8, 4)),
            u32::from_le_bytes(*array_ref!(bytes, 12, 4)),
        ])
    }
}

impl Row {
    #[inline(always)]
    fn empty() -> Self {
        Row([0; 4])
    }

    #[inline(always)]
    fn iter_mut(&mut self) -> IterMut<'_, u32> {
        self.0.iter_mut()
    }

    #[inline(always)]
    fn iter_ops<'rhs, 'lhs: 'rhs, 'a: 'lhs>(
        &'a mut self,
        lhs: &'lhs Self,
        rhs: &'lhs Self,
    ) -> IterOps<'a, 'lhs, 'rhs> {
        self.0
            .iter_mut()
            .zip(lhs.0.iter().zip(rhs.0.iter()))
            .map(map_out_lhs_rhs)
    }

    #[inline(always)]
    fn rot(&mut self, n: u32) {
        self.0[0] = self.0[0].rotate_right(n);
        self.0[1] = self.0[1].rotate_right(n);
        self.0[2] = self.0[2].rotate_right(n);
        self.0[3] = self.0[3].rotate_right(n);
        //for word in &mut self.0 {
        //    *word = word.rotate_right(n);
        //}
    }

    /// Equivalent to `_mm_shuffle_epi32(src, _MM_SHUFFLE!(z, y, x, w))`
    #[inline(always)]
    fn shuffle(row: &Self, z: usize, y: usize, x: usize, w: usize) -> Self {
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
    #[inline(always)]
    fn shuffle2(lhs: &Self, rhs: &Self, z: usize, y: usize, x: usize, w: usize) -> Self {
        let (lhs, rhs) = (lhs.0, rhs.0);
        Self([lhs[w], lhs[x], rhs[y], rhs[z]])
    }
}

#[inline(always)]
fn map_out_lhs_rhs<'a>(
    (out, (&lhs, &rhs)): (&'a mut u32, (&u32, &u32)),
) -> (&'a mut u32, u32, u32) {
    (out, lhs, rhs)
}

macro_rules! r {
    ($row:expr, $idx:expr) => {
        $row.0[$idx]
    };
}

impl Add<&Row> for &Row {
    type Output = Row;

    #[inline(always)]
    fn add(self, rhs: &Row) -> Self::Output {
        let mut out = Row([0; 4]);
        r!(out, 0) = r!(self, 0).wrapping_add(r!(rhs, 0));
        r!(out, 1) = r!(self, 1).wrapping_add(r!(rhs, 1));
        r!(out, 2) = r!(self, 2).wrapping_add(r!(rhs, 2));
        r!(out, 3) = r!(self, 3).wrapping_add(r!(rhs, 3));

        /*for (out, (lhs, rhs)) in out.0.iter_mut().zip(self.0.iter().zip(rhs.0)) {
            *out = lhs.wrapping_add(rhs)
        }*/
        out
    }
}

impl AddAssign<&Row> for Row {
    #[inline(always)]
    fn add_assign(&mut self, rhs: &Row) {
        r!(self, 0) = r!(self, 0).wrapping_add(r!(rhs, 0));
        r!(self, 1) = r!(self, 1).wrapping_add(r!(rhs, 1));
        r!(self, 2) = r!(self, 2).wrapping_add(r!(rhs, 2));
        r!(self, 3) = r!(self, 3).wrapping_add(r!(rhs, 3));

        //for (lhs, rhs) in self.0.iter_mut().zip(rhs.0) {
        //    *lhs = lhs.wrapping_add(rhs)
        //}
    }
}

impl BitXorAssign<&Row> for Row {
    #[inline(always)]
    fn bitxor_assign(&mut self, rhs: &Row) {
        r!(self, 0) ^= r!(rhs, 0);
        r!(self, 1) ^= r!(rhs, 1);
        r!(self, 2) ^= r!(rhs, 2);
        r!(self, 3) ^= r!(rhs, 3);

        //for (lhs, rhs) in self.0.iter_mut().zip(rhs.0) {
        //    *lhs ^= rhs
        //}
    }
}

impl BitAnd<Self> for &Row {
    type Output = Row;

    #[inline(always)]
    fn bitand(self, rhs: Self) -> Self::Output {
        let mut out = Row::empty();
        r!(out, 0) = r!(self, 0) & r!(rhs, 0);
        r!(out, 1) = r!(self, 1) & r!(rhs, 1);
        r!(out, 2) = r!(self, 2) & r!(rhs, 2);
        r!(out, 3) = r!(self, 3) & r!(rhs, 3);
        //for (out, lhs, rhs) in out.iter_ops(self, rhs) {
        //    *out = lhs & rhs
        //}
        out
    }
}

impl BitOr<Self> for Row {
    type Output = Self;

    #[inline(always)]
    fn bitor(mut self, rhs: Self) -> Self::Output {
        r!(self, 0) |= r!(rhs, 0);
        r!(self, 1) |= r!(rhs, 1);
        r!(self, 2) |= r!(rhs, 2);
        r!(self, 3) |= r!(rhs, 3);
        //for (lhs, rhs) in self.0.iter_mut().zip(rhs.0) {
        //    *lhs |= rhs;
        //}
        self
    }
}

impl Not for &Row {
    type Output = Row;

    #[inline(always)]
    fn not(self) -> Self::Output {
        let mut out = Row::empty();
        r!(out, 0) = !r!(self, 0);
        r!(out, 1) = !r!(self, 1);
        r!(out, 2) = !r!(self, 2);
        r!(out, 3) = !r!(self, 3);
        //for (out, word) in out.iter_mut().zip(self.0) {
        //    *out = !word
        //}
        out
    }
}

#[inline(always)]
fn g1(row0: &mut Row, row1: &mut Row, row2: &mut Row, row3: &mut Row, m: &Row) {
    *row0 = row0.add(m).add(row1);
    row3.bitxor_assign(row0);
    row3.rot(16);
    row2.add_assign(row3);
    row1.bitxor_assign(row2);
    row1.rot(12)
}

#[inline(always)]
fn g2(row0: &mut Row, row1: &mut Row, row2: &mut Row, row3: &mut Row, m: &Row) {
    *row0 = row0.add(m).add(row1);
    row3.bitxor_assign(row0);
    row3.rot(8);
    row2.add_assign(row3);
    row1.bitxor_assign(row2);
    row1.rot(7)
}

// Note the optimization here of leaving row1 as the unrotated row, rather than
// row0. All the message loads below are adjusted to compensate for this. See
// discussion at https://github.com/sneves/blake2-avx2/pull/4

#[inline(always)]
fn diagonalize(row0: &mut Row, row2: &mut Row, row3: &mut Row) {
    *row0 = Row::shuffle(row0, 2, 1, 0, 3);
    *row3 = Row::shuffle(row3, 1, 0, 3, 2);
    *row2 = Row::shuffle(row2, 0, 3, 2, 1);
}

#[inline(always)]
fn undiagonalize(row0: &mut Row, row2: &mut Row, row3: &mut Row) {
    *row0 = Row::shuffle(row0, 0, 3, 2, 1);
    *row3 = Row::shuffle(row3, 1, 0, 3, 2);
    *row2 = Row::shuffle(row2, 2, 1, 0, 3);
}

macro_rules! cmpeq_epi16 {
    ($out:expr, $mask:expr, $bits:expr, $offset:literal) => {
        let [m0, m1, m2, m3] = $mask.0[$offset].to_le_bytes();
        let [b0, b1, b2, b3] = $bits.0[$offset].to_le_bytes();

        let first = cmp_u16(m0, m1, b0, b1);
        let second = cmp_u16(m2, m3, b2, b3);

        let idx = $offset * 4;
        $out[idx] = first;
        $out[idx + 1] = first;
        $out[idx + 2] = second;
        $out[idx + 3] = second;
    };
}

#[inline(always)]
fn blend_epi16(lhs: &Row, rhs: &Row, imm8: i32) -> Row {
    let bits = Row([0x0001_0002, 0x0004_0008, 0x0010_0020, 0x0040_0080]);
    let mut mask = {
        let bytes = (imm8 as i16).to_le_bytes();
        let imm8_32 = u32::from_le_bytes([bytes[0], bytes[1], bytes[0], bytes[1]]);
        Row([imm8_32, imm8_32, imm8_32, imm8_32])
    };

    mask = &mask & &bits;

    // _mm_cmpeq_epi16
    mask = {
        // todo: chunk
        let mut out = [0u8; 16];
        cmpeq_epi16!(out, mask, bits, 0);
        cmpeq_epi16!(out, mask, bits, 1);
        cmpeq_epi16!(out, mask, bits, 2);
        cmpeq_epi16!(out, mask, bits, 3);
        Row::from(&out)
    };

    (&mask & rhs) | (&!&mask & lhs)
}

#[inline(always)]
fn cmp_u16(l0: u8, l1: u8, r0: u8, r1: u8) -> u8 {
    if l0 == r0 && l1 == r1 {
        0xFF
    } else {
        0x00
    }
}

#[inline(always)]
fn unpacklo_epi64(lhs: &Row, rhs: &Row) -> Row {
    Row([lhs.0[0], lhs.0[1], rhs.0[0], rhs.0[1]])
}

#[inline(always)]
fn unpacklo_epi32(lhs: &Row, rhs: &Row) -> Row {
    Row([lhs.0[0], rhs.0[0], lhs.0[1], rhs.0[1]])
}

#[inline(always)]
fn unpackhi_epi32(lhs: &Row, rhs: &Row) -> Row {
    Row([lhs.0[2], rhs.0[2], lhs.0[3], rhs.0[3]])
}

macro_rules! round2plus {
    (
        $row0:ident, $row1:ident, $row2:ident, $row3:ident,
        $m0:ident,$m1:ident, $m2:ident, $m3:ident,
        $t0:ident, $t1:ident, $t2:ident, $t3:ident,
        $tt:ident
    ) => {
        $t0 = Row::shuffle2(&$m0, &$m1, 3, 1, 1, 2);
        $t0 = Row::shuffle(&$t0, 0, 3, 2, 1);
        g1(&mut $row0, &mut $row1, &mut $row2, &mut $row3, &$t0);
        $t1 = Row::shuffle2(&$m2, &$m3, 3, 3, 2, 2);
        $tt = Row::shuffle(&$m0, 0, 0, 3, 3);
        $t1 = blend_epi16(&$tt, &$t1, 0xCC);
        g2(&mut $row0, &mut $row1, &mut $row2, &mut $row3, &$t1);
        diagonalize(&mut $row0, &mut $row2, &mut $row3);
        $t2 = unpacklo_epi64(&$m3, &$m1);
        $tt = blend_epi16(&$t2, &$m2, 0xC0);
        $t2 = Row::shuffle(&$tt, 1, 3, 2, 0);
        g1(&mut $row0, &mut $row1, &mut $row2, &mut $row3, &$t2);
        $t3 = unpackhi_epi32(&$m1, &$m3);
        $tt = unpacklo_epi32(&$m2, &$t3);
        $t3 = Row::shuffle(&$tt, 0, 1, 3, 2);
        g2(&mut $row0, &mut $row1, &mut $row2, &mut $row3, &$t3);
        undiagonalize(&mut $row0, &mut $row2, &mut $row3);
    };
}

fn compress_pre(
    cv: &CVWords,
    block: &[u8; BLOCK_LEN],
    block_len: u8,
    counter: u64,
    flags: u8,
) -> [u32; 16] {
    let mut row0 = Row::from([cv[0], cv[1], cv[2], cv[3]]);
    let mut row1 = Row::from([cv[4], cv[5], cv[6], cv[7]]);
    let mut row2 = Row::from([IV[0], IV[1], IV[2], IV[3]]);
    let mut row3 = Row::from([
        counter_low(counter),
        counter_high(counter),
        block_len as u32,
        flags as u32,
    ]);

    let mut m0 = Row::from(array_ref!(block, 0, 16));
    let mut m1 = Row::from(array_ref!(block, 16, 16));
    let mut m2 = Row::from(array_ref!(block, 32, 16));
    let mut m3 = Row::from(array_ref!(block, 48, 16));

    // we only use it from the macro, but want it existing for all of them
    #[allow(clippy::needless_late_init)]
    let mut tt;

    // Round 1. The first round permutes the message words from the original
    // input order, into the groups that get mixed in parallel.
    let mut t0 = Row::shuffle2(&m0, &m1, 2, 0, 2, 0);
    g1(&mut row0, &mut row1, &mut row2, &mut row3, &t0);
    let mut t1 = Row::shuffle2(&m0, &m1, 3, 1, 3, 1);
    g2(&mut row0, &mut row1, &mut row2, &mut row3, &t1);
    diagonalize(&mut row0, &mut row2, &mut row3);
    let mut t2 = Row::shuffle2(&m2, &m3, 2, 0, 2, 0);
    t2 = Row::shuffle(&t2, 2, 1, 0, 3);
    g1(&mut row0, &mut row1, &mut row2, &mut row3, &t2);
    let mut t3 = Row::shuffle2(&m2, &m3, 3, 1, 3, 1);
    t3 = Row::shuffle(&t3, 2, 1, 0, 3);
    g2(&mut row0, &mut row1, &mut row2, &mut row3, &t3);
    undiagonalize(&mut row0, &mut row2, &mut row3);
    m0 = t0;
    m1 = t1;
    m2 = t2;
    m3 = t3;

    // Round 2. This round and all following rounds apply a fixed permutation
    // to the message words from the round before.
    round2plus!(row0, row1, row2, row3, m0, m1, m2, m3, t0, t1, t2, t3, tt);
    m0 = t0;
    m1 = t1;
    m2 = t2;
    m3 = t3;

    // round 3
    round2plus!(row0, row1, row2, row3, m0, m1, m2, m3, t0, t1, t2, t3, tt);
    m0 = t0;
    m1 = t1;
    m2 = t2;
    m3 = t3;

    // round 4
    round2plus!(row0, row1, row2, row3, m0, m1, m2, m3, t0, t1, t2, t3, tt);
    m0 = t0;
    m1 = t1;
    m2 = t2;
    m3 = t3;

    // round 5
    round2plus!(row0, row1, row2, row3, m0, m1, m2, m3, t0, t1, t2, t3, tt);
    m0 = t0;
    m1 = t1;
    m2 = t2;
    m3 = t3;

    // round 6
    round2plus!(row0, row1, row2, row3, m0, m1, m2, m3, t0, t1, t2, t3, tt);
    m0 = t0;
    m1 = t1;
    m2 = t2;
    m3 = t3;

    // round 7
    round2plus!(row0, row1, row2, row3, m0, m1, m2, m3, t0, t1, t2, t3, tt);

    [
        row0.0[0], row0.0[1], row0.0[2], row0.0[3], row1.0[0], row1.0[1], row1.0[2], row1.0[3],
        row2.0[0], row2.0[1], row2.0[2], row2.0[3], row3.0[0], row3.0[1], row3.0[2], row3.0[3],
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
            array_ref_mut!(output, 0, OUT_LEN),
        );
    }
}

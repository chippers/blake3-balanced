//! This is the reference implementation of BLAKE3. It is used for testing and
//! as a readable example of the algorithms involved. Section 5.1 of [the BLAKE3
//! spec](https://github.com/BLAKE3-team/BLAKE3-specs/blob/master/blake3.pdf)
//! discusses this implementation. You can render docs for this implementation
//! by running `cargo doc --open` in this directory.
//!
//! # Example
//!
//! ```no_run
//! let mut hasher = blake3_balanced::Hasher::new();
//! hasher.update(b"abc");
//! hasher.update(b"def");
//! //let mut hash = [0; 32];
//! //hasher.finalize(&mut hash);
//! //let mut extended_hash = [0; 500];
//! //hasher.finalize(&mut extended_hash);
//! //assert_eq!(hash, extended_hash[..32]);
//! ```
//!

//#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

use crate::platform::{array_ref, array_ref_mut};
use core::{cmp, fmt};

mod compress;
mod join;
mod platform;

// While iterating the compression function within a chunk, the CV is
// represented as words, to avoid doing two extra endianness conversions for
// each compression in the portable implementation. But the hash_many interface
// needs to hash both input bytes and parent nodes, so its better for its
// output CVs to be represented as bytes.
type CVWords = [u32; 8];
type CVBytes = [u8; 32]; // little-endian

const IV: &CVWords = &[
    0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19,
];

// These are the internal flags that we use to domain separate root/non-root,
// chunk/parent, and chunk beginning/middle/end. These get set at the high end
// of the block flags word in the compression function, so their values start
// high and go down.
const CHUNK_START: u8 = 1 << 0;
const CHUNK_END: u8 = 1 << 1;
const PARENT: u8 = 1 << 2;
const ROOT: u8 = 1 << 3;
const KEYED_HASH: u8 = 1 << 4;
const DERIVE_KEY_CONTEXT: u8 = 1 << 5;
const DERIVE_KEY_MATERIAL: u8 = 1 << 6;

pub const BLOCK_LEN: usize = 64;
pub const CHUNK_LEN: usize = 1024;

/// The number of bytes in a [`Hash`](struct.Hash.html), 32.
pub const OUT_LEN: usize = 32;

/// The number of bytes in a key, 32.
pub const KEY_LEN: usize = 32;

const MAX_DEPTH: usize = 54; // 2^54 * CHUNK_LEN = 2^64

#[inline]
fn counter_low(counter: u64) -> u32 {
    counter as u32
}

#[inline]
fn counter_high(counter: u64) -> u32 {
    (counter >> 32) as u32
}

pub struct Hash([u8; OUT_LEN]);

impl Hash {
    /// The raw bytes of the `Hash`. Note that byte arrays don't provide
    /// constant-time equality checking, so if  you need to compare hashes,
    /// prefer the `Hash` type.
    #[inline]
    pub fn as_bytes(&self) -> &[u8; OUT_LEN] {
        &self.0
    }
}

// Each chunk or parent node can produce either a 32-byte chaining value or, by
// setting the ROOT flag, any number of final output bytes. The Output struct
// captures the state just prior to choosing between those two possibilities.
#[derive(Clone)]
struct Output {
    input_chaining_value: CVWords,
    block: [u8; 64],
    block_len: u8,
    counter: u64,
    flags: u8,
}

impl Output {
    fn chaining_value(&self) -> CVBytes {
        let mut cv = self.input_chaining_value;
        compress::compress_in_place(
            &mut cv,
            &self.block,
            self.block_len,
            self.counter,
            self.flags,
        );
        platform::le_bytes_from_words_32(&cv)
    }

    fn root_hash(&self) -> Hash {
        debug_assert_eq!(self.counter, 0);
        let mut cv = self.input_chaining_value;
        compress::compress_in_place(&mut cv, &self.block, self.block_len, 0, self.flags | ROOT);
        Hash(platform::le_bytes_from_words_32(&cv))
    }
}

#[derive(Clone)]
struct ChunkState {
    cv: CVWords,
    chunk_counter: u64,
    buf: [u8; BLOCK_LEN],
    buf_len: u8,
    blocks_compressed: u8,
    flags: u8,
}

impl ChunkState {
    fn new(key: &CVWords, chunk_counter: u64, flags: u8) -> Self {
        Self {
            cv: *key,
            chunk_counter,
            buf: [0; BLOCK_LEN],
            buf_len: 0,
            blocks_compressed: 0,
            flags,
        }
    }

    fn len(&self) -> usize {
        BLOCK_LEN * self.blocks_compressed as usize + self.buf_len as usize
    }

    fn fill_buf(&mut self, input: &mut &[u8]) {
        let want = BLOCK_LEN - self.buf_len as usize;
        let take = cmp::min(want, input.len());
        self.buf[self.buf_len as usize..][..take].copy_from_slice(&input[..take]);
        self.buf_len += take as u8;
        *input = &input[take..];
    }

    fn start_flag(&self) -> u8 {
        if self.blocks_compressed == 0 {
            CHUNK_START
        } else {
            0
        }
    }

    // Try to avoid buffering as much as possible, by compressing directly from
    // the input slice when full blocks are available.
    fn update(&mut self, mut input: &[u8]) -> &mut Self {
        if self.buf_len > 0 {
            self.fill_buf(&mut input);
            if !input.is_empty() {
                debug_assert_eq!(self.buf_len as usize, BLOCK_LEN);
                let block_flags = self.flags | self.start_flag(); // borrowck
                compress::compress_in_place(
                    &mut self.cv,
                    &self.buf,
                    BLOCK_LEN as u8,
                    self.chunk_counter,
                    block_flags,
                );
                self.buf_len = 0;
                self.buf = [0; BLOCK_LEN];
                self.blocks_compressed += 1;
            }
        }

        while input.len() > BLOCK_LEN {
            debug_assert_eq!(self.buf_len, 0);
            let block_flags = self.flags | self.start_flag(); // borrowck
            compress::compress_in_place(
                &mut self.cv,
                array_ref!(input, 0, BLOCK_LEN),
                BLOCK_LEN as u8,
                self.chunk_counter,
                block_flags,
            );
            self.blocks_compressed += 1;
            input = &input[BLOCK_LEN..];
        }

        self.fill_buf(&mut input);
        debug_assert!(input.is_empty());
        debug_assert!(self.len() <= CHUNK_LEN);
        self
    }

    fn output(&self) -> Output {
        let block_flags = self.flags | self.start_flag() | CHUNK_END;
        Output {
            input_chaining_value: self.cv,
            block: self.buf,
            block_len: self.buf_len,
            counter: self.chunk_counter,
            flags: block_flags,
        }
    }
}

// Don't derive(Debug), because the state may be secret.
impl fmt::Debug for ChunkState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ChunkState")
            .field("len", &self.len())
            .field("chunk_counter", &self.chunk_counter)
            .field("flags", &self.flags)
            .finish()
    }
}

// IMPLEMENTATION NOTE
// ===================
// The recursive function compress_subtree_wide(), implemented below, is the
// basis of high-performance BLAKE3. We use it both for all-at-once hashing,
// and for the incremental input with Hasher (though we have to be careful with
// subtree boundaries in the incremental case). compress_subtree_wide() applies
// several optimizations at the same time:
// - Multithreading with Rayon.
// - Parallel chunk hashing with SIMD.
// - Parallel parent hashing with SIMD. Note that while SIMD chunk hashing
//   maxes out at MAX_SIMD_DEGREE*CHUNK_LEN, parallel parent hashing continues
//   to benefit from larger inputs, because more levels of the tree benefit can
//   use full-width SIMD vectors for parent hashing. Without parallel parent
//   hashing, we lose about 10% of overall throughput on AVX2 and AVX-512.

/// Undocumented and unstable, for benchmarks only.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub enum IncrementCounter {
    Yes,
    No,
}

// The largest power of two less than or equal to `n`, used for left_len()
// immediately below, and also directly in Hasher::update().
fn largest_power_of_two_leq(n: usize) -> usize {
    ((n / 2) + 1).next_power_of_two()
}

// Given some input larger than one chunk, return the number of bytes that
// should go in the left subtree. This is the largest power-of-2 number of
// chunks that leaves at least 1 byte for the right subtree.
fn left_len(content_len: usize) -> usize {
    debug_assert!(content_len > CHUNK_LEN);
    // Subtract 1 to reserve at least one byte for the right side.
    let full_chunks = (content_len - 1) / CHUNK_LEN;
    largest_power_of_two_leq(full_chunks) * CHUNK_LEN
}

pub const MAX_SIMD_DEGREE: usize = 1;
pub const MAX_SIMD_DEGREE_OR_2: usize = 2;

// Use SIMD parallelism to hash up to MAX_SIMD_DEGREE chunks at the same time
// on a single thread. Write out the chunk chaining values and return the
// number of chunks hashed. These chunks are never the root and never empty;
// those cases use a different codepath.
fn compress_chunks_parallel(
    input: &[u8],
    key: &CVWords,
    chunk_counter: u64,
    flags: u8,
    out: &mut [u8],
) -> usize {
    debug_assert!(!input.is_empty(), "empty chunks below the root");
    debug_assert!(input.len() <= MAX_SIMD_DEGREE * CHUNK_LEN);

    let mut chunks_exact = input.chunks_exact(CHUNK_LEN);
    let mut chunks_array = [&[0u8; CHUNK_LEN]; MAX_SIMD_DEGREE];
    let mut chunks_array_len = 0;
    for chunk in &mut chunks_exact {
        chunks_array[chunks_array_len] = array_ref!(chunk, 0, CHUNK_LEN);
        chunks_array_len += 1;
    }
    compress::hash_many(
        &chunks_array[..chunks_array_len],
        key,
        chunk_counter,
        flags,
        CHUNK_START,
        CHUNK_END,
        out,
    );

    // Hash the remaining partial chunk, if there is one. Note that the empty
    // chunk (meaning the empty message) is a different codepath.
    let chunks_so_far = chunks_array_len;
    if !chunks_exact.remainder().is_empty() {
        let counter = chunk_counter + chunks_so_far as u64;
        let mut chunk_state = ChunkState::new(key, counter, flags);
        chunk_state.update(chunks_exact.remainder());
        *array_ref_mut!(out, chunks_so_far * OUT_LEN, OUT_LEN) =
            chunk_state.output().chaining_value();
        chunks_so_far + 1
    } else {
        chunks_so_far
    }
}

// Use SIMD parallelism to hash up to MAX_SIMD_DEGREE parents at the same time
// on a single thread. Write out the parent chaining values and return the
// number of parents hashed. (If there's an odd input chaining value left over,
// return it as an additional output.) These parents are never the root and
// never empty; those cases use a different codepath.
fn compress_parents_parallel(
    child_chaining_values: &[u8],
    key: &CVWords,
    flags: u8,
    out: &mut [u8],
) -> usize {
    debug_assert_eq!(child_chaining_values.len() % OUT_LEN, 0, "wacky hash bytes");
    let num_children = child_chaining_values.len() / OUT_LEN;
    debug_assert!(num_children >= 2, "not enough children");
    debug_assert!(num_children <= 2 * MAX_SIMD_DEGREE_OR_2, "too many");

    let mut parents_exact = child_chaining_values.chunks_exact(BLOCK_LEN);
    // Use MAX_SIMD_DEGREE_OR_2 rather than MAX_SIMD_DEGREE here, because of
    // the requirements of compress_subtree_wide().
    let mut parents_array = [&[0u8; BLOCK_LEN]; MAX_SIMD_DEGREE_OR_2];
    let mut parents_array_len = 0;
    for parent in &mut parents_exact {
        parents_array[parents_array_len] = array_ref!(parent, 0, BLOCK_LEN);
        parents_array_len += 1;
    }
    compress::hash_many(
        &parents_array[..parents_array_len],
        key,
        0, // Parents always use counter 0.
        flags | PARENT,
        0, // Parents have no start flags.
        0, // Parents have no end flags.
        out,
    );

    // If there's an odd child left over, it becomes an output.
    let parents_so_far = parents_array_len;
    if !parents_exact.remainder().is_empty() {
        out[parents_so_far * OUT_LEN..][..OUT_LEN].copy_from_slice(parents_exact.remainder());
        parents_so_far + 1
    } else {
        parents_so_far
    }
}

// The wide helper function returns (writes out) an array of chaining values
// and returns the length of that array. The number of chaining values returned
// is the dyanmically detected SIMD degree, at most MAX_SIMD_DEGREE. Or fewer,
// if the input is shorter than that many chunks. The reason for maintaining a
// wide array of chaining values going back up the tree, is to allow the
// implementation to hash as many parents in parallel as possible.
//
// As a special case when the SIMD degree is 1, this function will still return
// at least 2 outputs. This guarantees that this function doesn't perform the
// root compression. (If it did, it would use the wrong flags, and also we
// wouldn't be able to implement exendable output.) Note that this function is
// not used when the whole input is only 1 chunk long; that's a different
// codepath.
//
// Why not just have the caller split the input on the first update(), instead
// of implementing this special rule? Because we don't want to limit SIMD or
// multithreading parallelism for that update().
fn compress_subtree_wide<J: join::Join>(
    input: &[u8],
    key: &CVWords,
    chunk_counter: u64,
    flags: u8,
    out: &mut [u8],
) -> usize {
    // Note that the single chunk case does *not* bump the SIMD degree up to 2
    // when it is 1. This allows Rayon the option of multithreading even the
    // 2-chunk case, which can help performance on smaller platforms.
    if input.len() <= MAX_SIMD_DEGREE * CHUNK_LEN {
        return compress_chunks_parallel(input, key, chunk_counter, flags, out);
    }

    // With more than simd_degree chunks, we need to recurse. Start by dividing
    // the input into left and right subtrees. (Note that this is only optimal
    // as long as the SIMD degree is a power of 2. If we ever get a SIMD degree
    // of 3 or something, we'll need a more complicated strategy.)
    debug_assert_eq!(MAX_SIMD_DEGREE.count_ones(), 1, "power of 2");
    let (left, right) = input.split_at(left_len(input.len()));
    let right_chunk_counter = chunk_counter + (left.len() / CHUNK_LEN) as u64;

    // Make space for the child outputs. Here we use MAX_SIMD_DEGREE_OR_2 to
    // account for the special case of returning 2 outputs when the SIMD degree
    // is 1.
    let mut cv_array = [0; 2 * MAX_SIMD_DEGREE_OR_2 * OUT_LEN];
    let degree = if left.len() == CHUNK_LEN {
        // The "simd_degree=1 and we're at the leaf nodes" case.
        debug_assert_eq!(MAX_SIMD_DEGREE, 1);
        1
    } else {
        cmp::max(MAX_SIMD_DEGREE, 2)
    };
    let (left_out, right_out) = cv_array.split_at_mut(degree * OUT_LEN);

    // Recurse! For update_rayon(), this is where we take advantage of RayonJoin and use multiple
    // threads.
    let (left_n, right_n) = J::join(
        || compress_subtree_wide::<J>(left, key, chunk_counter, flags, left_out),
        || compress_subtree_wide::<J>(right, key, right_chunk_counter, flags, right_out),
    );

    // The special case again. If simd_degree=1, then we'll have left_n=1 and
    // right_n=1. Rather than compressing them into a single output, return
    // them directly, to make sure we always have at least two outputs.
    debug_assert_eq!(left_n, degree);
    debug_assert!(right_n >= 1 && right_n <= left_n);
    if left_n == 1 {
        out[..2 * OUT_LEN].copy_from_slice(&cv_array[..2 * OUT_LEN]);
        return 2;
    }

    // Otherwise, do one layer of parent node compression.
    let num_children = left_n + right_n;
    compress_parents_parallel(&cv_array[..num_children * OUT_LEN], key, flags, out)
}

// Hash a subtree with compress_subtree_wide(), and then condense the resulting
// list of chaining values down to a single parent node. Don't compress that
// last parent node, however. Instead, return its message bytes (the
// concatenated chaining values of its children). This is necessary when the
// first call to update() supplies a complete subtree, because the topmost
// parent node of that subtree could end up being the root. It's also necessary
// for extended output in the general case.
//
// As with compress_subtree_wide(), this function is not used on inputs of 1
// chunk or less. That's a different codepath.
fn compress_subtree_to_parent_node<J: join::Join>(
    input: &[u8],
    key: &CVWords,
    chunk_counter: u64,
    flags: u8,
) -> [u8; BLOCK_LEN] {
    debug_assert!(input.len() > CHUNK_LEN);
    let mut cv_array = [0; MAX_SIMD_DEGREE_OR_2 * OUT_LEN];
    let mut num_cvs = compress_subtree_wide::<J>(input, key, chunk_counter, flags, &mut cv_array);
    debug_assert!(num_cvs >= 2);

    // If MAX_SIMD_DEGREE is greater than 2 and there's enough input,
    // compress_subtree_wide() returns more than 2 chaining values. Condense
    // them into 2 by forming parent nodes repeatedly.
    let mut out_array = [0; MAX_SIMD_DEGREE_OR_2 * OUT_LEN / 2];
    while num_cvs > 2 {
        let cv_slice = &cv_array[..num_cvs * OUT_LEN];
        num_cvs = compress_parents_parallel(cv_slice, key, flags, &mut out_array);
        cv_array[..num_cvs * OUT_LEN].copy_from_slice(&out_array[..num_cvs * OUT_LEN]);
    }
    *array_ref!(cv_array, 0, 2 * OUT_LEN)
}

// Hash a complete input all at once. Unlike compress_subtree_wide() and
// compress_subtree_to_parent_node(), this function handles the 1 chunk case.
fn hash_all_at_once<J: join::Join>(input: &[u8], key: &CVWords, flags: u8) -> Output {
    // If the whole subtree is one chunk, hash it directly with a ChunkState.
    if input.len() <= CHUNK_LEN {
        return ChunkState::new(key, 0, flags).update(input).output();
    }

    // Otherwise construct an Output object from the parent node returned by
    // compress_subtree_to_parent_node().
    Output {
        input_chaining_value: *key,
        block: compress_subtree_to_parent_node::<J>(input, key, 0, flags),
        block_len: BLOCK_LEN as u8,
        counter: 0,
        flags: flags | PARENT,
    }
}

/// The default hash function.
///
/// For an incremental version that accepts multiple writes, see
/// [`Hasher::update`].
///
/// For output sizes other than 32 bytes, see [`Hasher::finalize_xof`] and
/// [`OutputReader`].
///
/// This function is always single-threaded. For multithreading support, see
/// [`Hasher::update_rayon`](struct.Hasher.html#method.update_rayon).
pub fn hash(input: &[u8]) -> Hash {
    hash_all_at_once::<join::SerialJoin>(input, IV, 0).root_hash()
}

/// The keyed hash function.
///
/// This is suitable for use as a message authentication code, for example to
/// replace an HMAC instance. In that use case, the constant-time equality
/// checking provided by [`Hash`](struct.Hash.html) is almost always a security
/// requirement, and callers need to be careful not to compare MACs as raw
/// bytes.
///
/// For output sizes other than 32 bytes, see [`Hasher::new_keyed`],
/// [`Hasher::finalize_xof`], and [`OutputReader`].
///
/// This function is always single-threaded. For multithreading support, see
/// [`Hasher::new_keyed`] and
/// [`Hasher::update_rayon`](struct.Hasher.html#method.update_rayon).
pub fn keyed_hash(key: &[u8; KEY_LEN], input: &[u8]) -> Hash {
    let key_words = platform::words_from_le_bytes_32(key);
    hash_all_at_once::<join::SerialJoin>(input, &key_words, KEYED_HASH).root_hash()
}

/// The key derivation function.
///
/// Given cryptographic key material of any length and a context string of any
/// length, this function outputs a 32-byte derived subkey. **The context string
/// should be hardcoded, globally unique, and application-specific.** A good
/// default format for such strings is `"[application] [commit timestamp]
/// [purpose]"`, e.g., `"example.com 2019-12-25 16:18:03 session tokens v1"`.
///
/// Key derivation is important when you want to use the same key in multiple
/// algorithms or use cases. Using the same key with different cryptographic
/// algorithms is generally forbidden, and deriving a separate subkey for each
/// use case protects you from bad interactions. Derived keys also mitigate the
/// damage from one part of your application accidentally leaking its key.
///
/// As a rare exception to that general rule, however, it is possible to use
/// `derive_key` itself with key material that you are already using with
/// another algorithm. You might need to do this if you're adding features to
/// an existing application, which does not yet use key derivation internally.
/// However, you still must not share key material with algorithms that forbid
/// key reuse entirely, like a one-time pad. For more on this, see sections 6.2
/// and 7.8 of the [BLAKE3 paper](https://github.com/BLAKE3-team/BLAKE3-specs/blob/master/blake3.pdf).
///
/// Note that BLAKE3 is not a password hash, and **`derive_key` should never be
/// used with passwords.** Instead, use a dedicated password hash like
/// [Argon2]. Password hashes are entirely different from generic hash
/// functions, with opposite design requirements.
///
/// For output sizes other than 32 bytes, see [`Hasher::new_derive_key`],
/// [`Hasher::finalize_xof`], and [`OutputReader`].
///
/// This function is always single-threaded. For multithreading support, see
/// [`Hasher::new_derive_key`] and
/// [`Hasher::update_rayon`](struct.Hasher.html#method.update_rayon).
///
/// [Argon2]: https://en.wikipedia.org/wiki/Argon2
pub fn derive_key(context: &str, key_material: &[u8]) -> [u8; OUT_LEN] {
    let context_key =
        hash_all_at_once::<join::SerialJoin>(context.as_bytes(), IV, DERIVE_KEY_CONTEXT)
            .root_hash();
    let context_key_words = platform::words_from_le_bytes_32(context_key.as_bytes());
    hash_all_at_once::<join::SerialJoin>(key_material, &context_key_words, DERIVE_KEY_MATERIAL)
        .root_hash()
        .0
}

fn parent_node_output(
    left_child: &CVBytes,
    right_child: &CVBytes,
    key: &CVWords,
    flags: u8,
) -> Output {
    let mut block = [0; BLOCK_LEN];
    block[..32].copy_from_slice(left_child);
    block[32..].copy_from_slice(right_child);
    Output {
        input_chaining_value: *key,
        block,
        block_len: BLOCK_LEN as u8,
        counter: 0,
        flags: flags | PARENT,
    }
}

/// An incremental hash state that can accept any number of writes.
///
/// When the `traits-preview` Cargo feature is enabled, this type implements
/// several commonly used traits from the
/// [`digest`](https://crates.io/crates/digest) crate. However, those
/// traits aren't stable, and they're expected to change in incompatible ways
/// before that crate reaches 1.0. For that reason, this crate makes no SemVer
/// guarantees for this feature, and callers who use it should expect breaking
/// changes between patch versions.
///
/// When the `rayon` Cargo feature is enabled, the
/// [`update_rayon`](#method.update_rayon) method is available for multithreaded
/// hashing.
///
/// **Performance note:** The [`update`](#method.update) method can't take full
/// advantage of SIMD optimizations if its input buffer is too small or oddly
/// sized. Using a 16 KiB buffer, or any multiple of that, enables all currently
/// supported SIMD instruction sets.
///
/// # Examples
///
/// ```
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // Hash an input incrementally.
/// let mut hasher = blake3::Hasher::new();
/// hasher.update(b"foo");
/// hasher.update(b"bar");
/// hasher.update(b"baz");
/// assert_eq!(hasher.finalize(), blake3::hash(b"foobarbaz"));
///
/// // Extended output. OutputReader also implements Read and Seek.
/// # #[cfg(feature = "std")] {
/// let mut output = [0; 1000];
/// let mut output_reader = hasher.finalize_xof();
/// output_reader.fill(&mut output);
/// assert_eq!(&output[..32], blake3::hash(b"foobarbaz").as_bytes());
/// # }
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Hasher {
    key: CVWords,
    chunk_state: ChunkState,
    // The stack size is MAX_DEPTH + 1 because we do lazy merging. For example,
    // with 7 chunks, we have 3 entries in the stack. Adding an 8th chunk
    // requires a 4th entry, rather than merging everything down to 1, because
    // we don't know whether more input is coming. This is different from how
    // the reference implementation does things.
    cv_stack: [CVBytes; MAX_DEPTH + 1],
    cv_stack_len: usize,
}

impl Hasher {
    fn new_internal(key: &CVWords, flags: u8) -> Self {
        Self {
            key: *key,
            chunk_state: ChunkState::new(key, 0, flags),
            cv_stack: [[0; 32]; MAX_DEPTH + 1],
            cv_stack_len: 0,
        }
    }

    /// Construct a new `Hasher` for the regular hash function.
    pub fn new() -> Self {
        Self::new_internal(IV, 0)
    }

    /// Construct a new `Hasher` for the keyed hash function. See
    /// [`keyed_hash`].
    ///
    /// [`keyed_hash`]: fn.keyed_hash.html
    pub fn new_keyed(key: &[u8; KEY_LEN]) -> Self {
        let key_words = platform::words_from_le_bytes_32(key);
        Self::new_internal(&key_words, KEYED_HASH)
    }

    /// Construct a new `Hasher` for the key derivation function. See
    /// [`derive_key`]. The context string should be hardcoded, globally
    /// unique, and application-specific.
    ///
    /// [`derive_key`]: fn.derive_key.html
    pub fn new_derive_key(context: &str) -> Self {
        let context_key =
            hash_all_at_once::<join::SerialJoin>(context.as_bytes(), IV, DERIVE_KEY_CONTEXT)
                .root_hash();
        let context_key_words = platform::words_from_le_bytes_32(context_key.as_bytes());
        Self::new_internal(&context_key_words, DERIVE_KEY_MATERIAL)
    }

    /// Reset the `Hasher` to its initial state.
    ///
    /// This is functionally the same as overwriting the `Hasher` with a new
    /// one, using the same key or context string if any.
    pub fn reset(&mut self) -> &mut Self {
        self.chunk_state = ChunkState::new(&self.key, 0, self.chunk_state.flags);
        for stack in self.cv_stack.iter_mut() {
            for byte in stack {
                *byte = 0
            }
        }
        self.cv_stack_len = 0;
        self
    }

    fn stack_push(&mut self, cv: CVBytes) {
        self.cv_stack[self.cv_stack_len] = cv;
        self.cv_stack_len += 1;
    }

    fn stack_pop(&mut self) -> CVBytes {
        self.cv_stack_len -= 1;
        self.cv_stack[self.cv_stack_len]
    }

    // As described in push_cv() below, we do "lazy merging", delaying merges
    // until right before the next CV is about to be added. This is different
    // from the reference implementation. Another difference is that we aren't
    // always merging 1 chunk at a time. Instead, each CV might represent any
    // power-of-two number of chunks, as long as the smaller-above-larger stack
    // order is maintained. Instead of the "count the trailing 0-bits"
    // algorithm described in the spec, we use a "count the total number of
    // 1-bits" variant that doesn't require us to retain the subtree size of
    // the CV on top of the stack. The principle is the same: each CV that
    // should remain in the stack is represented by a 1-bit in the total number
    // of chunks (or bytes) so far.
    fn merge_cv_stack(&mut self, total_len: u64) {
        let post_merge_stack_len = total_len.count_ones() as usize;
        while self.cv_stack_len > post_merge_stack_len {
            let right_child = self.stack_pop();
            let left_child = self.stack_pop();
            let parent_output =
                parent_node_output(&left_child, &right_child, &self.key, self.chunk_state.flags);
            self.stack_push(parent_output.chaining_value());
        }
    }

    // In reference_impl.rs, we merge the new CV with existing CVs from the
    // stack before pushing it. We can do that because we know more input is
    // coming, so we know none of the merges are root.
    //
    // This setting is different. We want to feed as much input as possible to
    // compress_subtree_wide(), without setting aside anything for the
    // chunk_state. If the user gives us 64 KiB, we want to parallelize over
    // all 64 KiB at once as a single subtree, if at all possible.
    //
    // This leads to two problems:
    // 1) This 64 KiB input might be the only call that ever gets made to
    //    update. In this case, the root node of the 64 KiB subtree would be
    //    the root node of the whole tree, and it would need to be ROOT
    //    finalized. We can't compress it until we know.
    // 2) This 64 KiB input might complete a larger tree, whose root node is
    //    similarly going to be the the root of the whole tree. For example,
    //    maybe we have 196 KiB (that is, 128 + 64) hashed so far. We can't
    //    compress the node at the root of the 256 KiB subtree until we know
    //    how to finalize it.
    //
    // The second problem is solved with "lazy merging". That is, when we're
    // about to add a CV to the stack, we don't merge it with anything first,
    // as the reference impl does. Instead we do merges using the *previous* CV
    // that was added, which is sitting on top of the stack, and we put the new
    // CV (unmerged) on top of the stack afterwards. This guarantees that we
    // never merge the root node until finalize().
    //
    // Solving the first problem requires an additional tool,
    // compress_subtree_to_parent_node(). That function always returns the top
    // *two* chaining values of the subtree it's compressing. We then do lazy
    // merging with each of them separately, so that the second CV will always
    // remain unmerged. (That also helps us support extendable output when
    // we're hashing an input all-at-once.)
    fn push_cv(&mut self, new_cv: &CVBytes, chunk_counter: u64) {
        self.merge_cv_stack(chunk_counter);
        self.stack_push(*new_cv);
    }

    /// Add input bytes to the hash state. You can call this any number of
    /// times.
    ///
    /// This method is always single-threaded. For multithreading support, see
    /// [`update_rayon`](#method.update_rayon) below (enabled with the `rayon`
    /// Cargo feature).
    ///
    /// Note that the degree of SIMD parallelism that `update` can use is
    /// limited by the size of this input buffer. The 8 KiB buffer currently
    /// used by [`std::io::copy`] is enough to leverage AVX2, for example, but
    /// not enough to leverage AVX-512. A 16 KiB buffer is large enough to
    /// leverage all currently supported SIMD instruction sets.
    ///
    /// [`std::io::copy`]: https://doc.rust-lang.org/std/io/fn.copy.html
    pub fn update(&mut self, input: &[u8]) -> &mut Self {
        self.update_with_join::<join::SerialJoin>(input)
    }

    #[cfg(feature = "rayon")]
    pub fn update_rayon(&mut self, input: &[u8]) -> &mut Self {
        self.update_with_join::<join::RayonJoin>(input)
    }

    fn update_with_join<J: join::Join>(&mut self, mut input: &[u8]) -> &mut Self {
        // If we have some partial chunk bytes in the internal chunk_state, we
        // need to finish that chunk first.
        if self.chunk_state.len() > 0 {
            let want = CHUNK_LEN - self.chunk_state.len();
            let take = cmp::min(want, input.len());
            self.chunk_state.update(&input[..take]);
            input = &input[take..];
            if !input.is_empty() {
                // We've filled the current chunk, and there's more input
                // coming, so we know it's not the root and we can finalize it.
                // Then we'll proceed to hashing whole chunks below.
                debug_assert_eq!(self.chunk_state.len(), CHUNK_LEN);
                let chunk_cv = self.chunk_state.output().chaining_value();
                self.push_cv(&chunk_cv, self.chunk_state.chunk_counter);
                self.chunk_state = ChunkState::new(
                    &self.key,
                    self.chunk_state.chunk_counter + 1,
                    self.chunk_state.flags,
                );
            } else {
                return self;
            }
        }

        // Now the chunk_state is clear, and we have more input. If there's
        // more than a single chunk (so, definitely not the root chunk), hash
        // the largest whole subtree we can, with the full benefits of SIMD and
        // multithreading parallelism. Two restrictions:
        // - The subtree has to be a power-of-2 number of chunks. Only subtrees
        //   along the right edge can be incomplete, and we don't know where
        //   the right edge is going to be until we get to finalize().
        // - The subtree must evenly divide the total number of chunks up until
        //   this point (if total is not 0). If the current incomplete subtree
        //   is only waiting for 1 more chunk, we can't hash a subtree of 4
        //   chunks. We have to complete the current subtree first.
        // Because we might need to break up the input to form powers of 2, or
        // to evenly divide what we already have, this part runs in a loop.
        while input.len() > CHUNK_LEN {
            debug_assert_eq!(self.chunk_state.len(), 0, "no partial chunk data");
            debug_assert_eq!(CHUNK_LEN.count_ones(), 1, "power of 2 chunk len");
            let mut subtree_len = largest_power_of_two_leq(input.len());
            let count_so_far = self.chunk_state.chunk_counter * CHUNK_LEN as u64;
            // Shrink the subtree_len until it evenly divides the count so far.
            // We know that subtree_len itself is a power of 2, so we can use a
            // bitmasking trick instead of an actual remainder operation. (Note
            // that if the caller consistently passes power-of-2 inputs of the
            // same size, as is hopefully typical, this loop condition will
            // always fail, and subtree_len will always be the full length of
            // the input.)
            //
            // An aside: We don't have to shrink subtree_len quite this much.
            // For example, if count_so_far is 1, we could pass 2 chunks to
            // compress_subtree_to_parent_node. Since we'll get 2 CVs back,
            // we'll still get the right answer in the end, and we might get to
            // use 2-way SIMD parallelism. The problem with this optimization,
            // is that it gets us stuck always hashing 2 chunks. The total
            // number of chunks will remain odd, and we'll never graduate to
            // higher degrees of parallelism. See
            // https://github.com/BLAKE3-team/BLAKE3/issues/69.
            while (subtree_len - 1) as u64 & count_so_far != 0 {
                subtree_len /= 2;
            }
            // The shrunken subtree_len might now be 1 chunk long. If so, hash
            // that one chunk by itself. Otherwise, compress the subtree into a
            // pair of CVs.
            let subtree_chunks = (subtree_len / CHUNK_LEN) as u64;
            if subtree_len <= CHUNK_LEN {
                debug_assert_eq!(subtree_len, CHUNK_LEN);
                self.push_cv(
                    &ChunkState::new(
                        &self.key,
                        self.chunk_state.chunk_counter,
                        self.chunk_state.flags,
                    )
                    .update(&input[..subtree_len])
                    .output()
                    .chaining_value(),
                    self.chunk_state.chunk_counter,
                );
            } else {
                // This is the high-performance happy path, though getting here
                // depends on the caller giving us a long enough input.
                let cv_pair = compress_subtree_to_parent_node::<J>(
                    &input[..subtree_len],
                    &self.key,
                    self.chunk_state.chunk_counter,
                    self.chunk_state.flags,
                );
                let left_cv = array_ref!(cv_pair, 0, 32);
                let right_cv = array_ref!(cv_pair, 32, 32);
                // Push the two CVs we received into the CV stack in order. Because
                // the stack merges lazily, this guarantees we aren't merging the
                // root.
                self.push_cv(left_cv, self.chunk_state.chunk_counter);
                self.push_cv(
                    right_cv,
                    self.chunk_state.chunk_counter + (subtree_chunks / 2),
                );
            }
            self.chunk_state.chunk_counter += subtree_chunks;
            input = &input[subtree_len..];
        }

        // What remains is 1 chunk or less. Add it to the chunk state.
        debug_assert!(input.len() <= CHUNK_LEN);
        if !input.is_empty() {
            self.chunk_state.update(input);
            // Having added some input to the chunk_state, we know what's in
            // the CV stack won't become the root node, and we can do an extra
            // merge. This simplifies finalize().
            self.merge_cv_stack(self.chunk_state.chunk_counter);
        }

        self
    }

    fn final_output(&self) -> Output {
        // If the current chunk is the only chunk, that makes it the root node
        // also. Convert it directly into an Output. Otherwise, we need to
        // merge subtrees below.
        if self.cv_stack_len == 0 {
            debug_assert_eq!(self.chunk_state.chunk_counter, 0);
            return self.chunk_state.output();
        }

        // If there are any bytes in the ChunkState, finalize that chunk and
        // merge its CV with everything in the CV stack. In that case, the work
        // we did at the end of update() above guarantees that the stack
        // doesn't contain any unmerged subtrees that need to be merged first.
        // (This is important, because if there were two chunk hashes sitting
        // on top of the stack, they would need to merge with each other, and
        // merging a new chunk hash into them would be incorrect.)
        //
        // If there are no bytes in the ChunkState, we'll merge what's already
        // in the stack. In this case it's fine if there are unmerged chunks on
        // top, because we'll merge them with each other. Note that the case of
        // the empty chunk is taken care of above.
        let mut output: Output;
        let mut num_cvs_remaining = self.cv_stack_len;
        if self.chunk_state.len() > 0 {
            debug_assert_eq!(
                self.cv_stack_len,
                self.chunk_state.chunk_counter.count_ones() as usize,
                "cv stack does not need a merge"
            );
            output = self.chunk_state.output();
        } else {
            debug_assert!(self.cv_stack_len >= 2);
            output = parent_node_output(
                &self.cv_stack[num_cvs_remaining - 2],
                &self.cv_stack[num_cvs_remaining - 1],
                &self.key,
                self.chunk_state.flags,
            );
            num_cvs_remaining -= 2;
        }
        while num_cvs_remaining > 0 {
            output = parent_node_output(
                &self.cv_stack[num_cvs_remaining - 1],
                &output.chaining_value(),
                &self.key,
                self.chunk_state.flags,
            );
            num_cvs_remaining -= 1;
        }
        output
    }

    /// Finalize the hash state and return the [`Hash`](struct.Hash.html) of
    /// the input.
    ///
    /// This method is idempotent. Calling it twice will give the same result.
    /// You can also add more input and finalize again.
    pub fn finalize(&self) -> Hash {
        self.final_output().root_hash()
    }

    /// Return the total number of bytes hashed so far.
    pub fn count(&self) -> u64 {
        self.chunk_state.chunk_counter * CHUNK_LEN as u64 + self.chunk_state.len() as u64
    }
}

// Don't derive(Debug), because the state may be secret.
impl fmt::Debug for Hasher {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Hasher")
            .field("flags", &self.chunk_state.flags)
            .finish()
    }
}

impl Default for Hasher {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    /// asserts that all implementations have the same output
    fn assert_implementation_output(data: &[u8]) {
        let standard = {
            let mut hasher = ::blake3::Hasher::default();
            hasher.update(data);
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
            let mut hasher = super::Hasher::new();
            hasher.update(data);
            *hasher.finalize().as_bytes()
        };

        assert_eq!(reference, standard);
        assert_eq!(reference, balanced);
    }

    #[test]
    fn small() {
        assert_implementation_output(b"small");
    }

    #[test]
    fn large() {
        let data = include_bytes!("../benches/element-web-v1.10.10-vendors~init.js");
        assert_implementation_output(data);
    }

    #[test]
    fn previous_fuzzing_fails() {
        let data = include_bytes!("../tests/data/fuzz_00");
        assert_implementation_output(data);

        let data = include_bytes!("../tests/data/fuzz_01");
        assert_implementation_output(data);

        let data = include_bytes!("../tests/data/fuzz_02");
        assert_implementation_output(data);

        let data = include_bytes!("../tests/data/fuzz_03");
        assert_implementation_output(data);

        let data = include_bytes!("../tests/data/fuzz_04");
        assert_implementation_output(data);
    }
}

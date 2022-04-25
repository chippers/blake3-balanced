/// The trait that abstracts over single-threaded and multi-threaded recursion.
///
/// See the [`join` module docs](index.html) for more details.
pub trait Join {
    fn join<A, B, RA, RB>(oper_a: A, oper_b: B) -> (RA, RB)
    where
        A: FnOnce() -> RA + Send,
        B: FnOnce() -> RB + Send,
        RA: Send,
        RB: Send;
}

/// The trivial, serial implementation of `Join`. The left and right sides are
/// executed one after the other, on the calling thread. The standalone hashing
/// functions and the `Hasher::update` method use this implementation
/// internally.
///
/// See the [`join` module docs](index.html) for more details.
pub enum SerialJoin {}

impl Join for SerialJoin {
    #[inline]
    fn join<A, B, RA, RB>(oper_a: A, oper_b: B) -> (RA, RB)
    where
        A: FnOnce() -> RA + Send,
        B: FnOnce() -> RB + Send,
        RA: Send,
        RB: Send,
    {
        (oper_a(), oper_b())
    }
}

/// The Rayon-based implementation of `Join`. The left and right sides are
/// executed on the Rayon thread pool, potentially in parallel. This
/// implementation is gated by the `rayon` feature, which is off by default.
///
/// See the [`join` module docs](index.html) for more details.
#[cfg(feature = "rayon")]
pub enum RayonJoin {}

#[cfg(feature = "rayon")]
impl Join for RayonJoin {
    #[inline]
    fn join<A, B, RA, RB>(oper_a: A, oper_b: B) -> (RA, RB)
    where
        A: FnOnce() -> RA + Send,
        B: FnOnce() -> RB + Send,
        RA: Send,
        RB: Send,
    {
        rayon::join(oper_a, oper_b)
    }
}

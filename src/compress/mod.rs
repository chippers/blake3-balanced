#[cfg(complex)]
mod complex;

#[cfg(not(complex))]
mod simple;

#[cfg(complex)]
pub use complex::{compress_in_place, hash_many};

#[cfg(not(complex))]
pub use simple::{compress_in_place, hash_many};

//! Deterministic gather-parallel helpers over hex indices.
//!
//! # Contract
//!
//! Closures passed to these helpers may **read** shared immutable inputs and must
//! **write only** to storage exclusive to index `i` (typically `out[i]`).
//!
//! Forbidden inside the parallel body:
//! - scatter writes (updating neighbor indices)
//! - unordered float reductions into a shared accumulator
//! - event emission / anything that depends on visit order
//!
//! With this contract, `RAYON_NUM_THREADS=1` and multi-thread runs produce
//! bit-identical simulation outputs.

use rayon::prelude::*;

/// Gather-parallel over hex indices `0..n`.
///
/// See module docs for the write/read contract.
pub fn par_for_each_hex(n: usize, f: impl Fn(usize) + Sync + Send) {
    (0..n).into_par_iter().for_each(f);
}

/// Gather-parallel mutate of a dense per-hex slice (`slice[i]` exclusive to `i`).
pub fn par_for_each_hex_mut<T: Send>(slice: &mut [T], f: impl Fn(usize, &mut T) + Sync + Send) {
    slice
        .par_iter_mut()
        .enumerate()
        .for_each(|(i, value)| f(i, value));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_indices_identically_serial_or_parallel() {
        let n = 10_000usize;
        let mut parallel = vec![0u32; n];
        par_for_each_hex_mut(&mut parallel, |i, v| *v = (i as u32).wrapping_mul(3));

        let serial: Vec<u32> = (0..n as u32).map(|i| i.wrapping_mul(3)).collect();
        assert_eq!(parallel, serial);
    }
}

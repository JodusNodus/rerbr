//! A tweak key's float triple, loaded from a binary block of four words.

use super::layout::{tweak_word_at, TWEAK_BLOCK_WORDS};
use crate::car_body::store::CarStore;

/// Reads the four words at `data` (all of them, before any store, as the original's paired 16-byte moves do) into the
/// triple at `triple`, and returns `triple`.
pub fn load_binary<S: CarStore>(mem: &mut S, triple: u32, data: u32) -> u32 {
    let words: [u32; TWEAK_BLOCK_WORDS] = std::array::from_fn(|i| mem.u32(tweak_word_at(data, i as u32)));
    for (i, word) in words.into_iter().enumerate() {
        mem.set_u32(tweak_word_at(triple, i as u32), word);
    }
    triple
}

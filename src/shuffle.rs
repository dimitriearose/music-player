//! Shuffling a list of songs ourselves, instead of using Music's shuffle.
//!
//! Why not just turn on Music's shuffle? Music decides the play order the
//! moment a playlist starts (DESIGN.md §12), so flipping shuffle on halfway
//! through often changes nothing you can hear. Shuffling the song list
//! *before* we build the queue always works, and the Queue tab then shows
//! the real order.
//!
//! We avoid pulling in a random-number crate for one small job: a tiny
//! "xorshift" generator seeded from the clock is plenty random for picking
//! a song order (it is *not* suitable for anything security-related).

use std::time::{SystemTime, UNIX_EPOCH};

/// A seed that's different every time: the current time in nanoseconds.
pub fn seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
}

/// Put `items` in a random order (the Fisher–Yates shuffle: walk from the
/// end, swapping each item with a random one at or before it). The same
/// seed always gives the same order, which makes it testable.
pub fn shuffle<T>(items: &mut [T], seed: u64) {
    // xorshift64 gets stuck at zero forever, so never start there.
    let mut state = seed | 1;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for i in (1..items.len()).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_every_item_exactly_once() {
        let mut v: Vec<u32> = (0..100).collect();
        shuffle(&mut v, 42);
        let mut sorted = v.clone();
        sorted.sort();
        assert_eq!(sorted, (0..100).collect::<Vec<_>>());
    }

    #[test]
    fn actually_changes_the_order_and_depends_on_the_seed() {
        let start: Vec<u32> = (0..50).collect();
        let (mut a, mut b) = (start.clone(), start.clone());
        shuffle(&mut a, 1);
        shuffle(&mut b, 2);
        assert_ne!(a, start);
        assert_ne!(a, b);
        // Same seed, same order.
        let mut c = start.clone();
        shuffle(&mut c, 1);
        assert_eq!(a, c);
    }

    #[test]
    fn handles_empty_and_single_lists() {
        let mut empty: Vec<u32> = vec![];
        shuffle(&mut empty, 7);
        let mut one = vec![5];
        shuffle(&mut one, 7);
        assert_eq!(one, [5]);
    }
}

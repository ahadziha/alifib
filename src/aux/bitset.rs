//! Dense bitset for mutable scratch state in the traversal algorithm.
//!
//! [`BitSet`] stores membership for a fixed universe `0..N` in packed `u64`
//! words.  It is used exclusively in `ogposet::traverse`, where several scratch
//! sets are pre-allocated once and then reused across loop iterations via
//! [`BitSet::reset`] and [`BitSet::copy_from`], avoiding per-iteration heap
//! allocation in the hot path.

/// Dense set of integers in a fixed universe `0..N`.
///
/// Representation:
/// - `bits[w]` stores membership for values `w*64 .. w*64+63`.
/// - value `x` is in the set iff bit `(x % 64)` of `bits[x / 64]` is `1`.
///
/// This is used for traversal scratch state where we need fast membership checks
/// and cheap word-level set operations.
#[derive(Clone)]
pub(crate) struct BitSet {
    bits:  Vec<u64>,
    count: usize,
}

impl BitSet {
    /// Allocate a bitset for the universe `0..universe`, with all bits clear.
    pub fn new(universe: usize) -> Self {
        let words = universe.div_ceil(64);
        BitSet { bits: vec![0u64; words], count: 0 }
    }

    /// Insert `x`; returns `true` if `x` was not already present.
    #[inline]
    pub fn insert(&mut self, x: usize) -> bool {
        let (w, b) = (x / 64, 1u64 << (x % 64));
        if self.bits[w] & b == 0 {
            self.bits[w] |= b;
            self.count += 1;
            true
        } else {
            false
        }
    }

    /// Remove `x`; returns `true` if `x` was present.
    #[inline]
    pub fn remove(&mut self, x: usize) -> bool {
        let (w, b) = (x / 64, 1u64 << (x % 64));
        if self.bits[w] & b != 0 {
            self.bits[w] &= !b;
            self.count -= 1;
            true
        } else {
            false
        }
    }

    /// True if `x` is in the set; safe to call with out-of-range values.
    #[inline]
    pub fn contains(&self, x: usize) -> bool {
        let w = x / 64;
        w < self.bits.len() && self.bits[w] & (1u64 << (x % 64)) != 0
    }

    /// True if no elements are present.
    pub fn is_empty(&self) -> bool { self.count == 0 }
    /// Number of elements currently in the set.
    pub fn len(&self) -> usize { self.count }

    /// Iterate over set members in ascending order.
    pub fn iter(&self) -> BitSetIter<'_> {
        BitSetIter {
            bits: &self.bits,
            word_idx: 0,
            word: self.bits.first().copied().unwrap_or(0),
        }
    }

    /// Zero all words and adjust length for a new universe size, reusing allocation.
    pub fn reset(&mut self, universe: usize) {
        let words_needed = universe.div_ceil(64);
        for w in self.bits.iter_mut() { *w = 0; }
        self.bits.resize(words_needed, 0);
        self.count = 0;
    }

    /// Copy contents from another BitSet, reusing existing allocation.
    pub fn copy_from(&mut self, other: &BitSet) {
        self.bits.clear();
        self.bits.extend_from_slice(&other.bits);
        self.count = other.count;
    }

    /// Return a new BitSet that is the union of self and other.
    pub fn union(&self, other: &BitSet) -> BitSet {
        let max_words = self.bits.len().max(other.bits.len());
        let mut bits = vec![0u64; max_words];
        for (i, w) in bits.iter_mut().enumerate() {
            let a = self.bits.get(i).copied().unwrap_or(0);
            let b = other.bits.get(i).copied().unwrap_or(0);
            *w = a | b;
        }
        let count = bits.iter().map(|w| w.count_ones() as usize).sum();
        BitSet { bits, count }
    }

    /// self &= !other  (in-place set-difference using word-level bitops)
    pub fn difference_inplace(&mut self, other: &BitSet) {
        let n = self.bits.len().min(other.bits.len());
        for i in 0..n {
            let removed = self.bits[i] & other.bits[i];
            self.bits[i] &= !other.bits[i];
            self.count -= removed.count_ones() as usize;
        }
    }
}

/// Iterator over the members of a [`BitSet`], yielding values in ascending order.
pub(crate) struct BitSetIter<'a> {
    bits:     &'a [u64],
    word_idx: usize,
    word:     u64,
}

impl<'a> Iterator for BitSetIter<'a> {
    type Item = usize;
    fn next(&mut self) -> Option<usize> {
        // Move to next non-empty word.
        while self.word == 0 {
            self.word_idx += 1;
            if self.word_idx >= self.bits.len() { return None; }
            self.word = self.bits[self.word_idx];
        }
        // Emit lowest set bit, then clear it so the next call finds the next one.
        let tz = self.word.trailing_zeros() as usize;
        self.word &= self.word - 1; // clear lowest set bit
        Some(self.word_idx * 64 + tz)
    }
}

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
    pub fn new(universe: usize) -> Self {
        let words = (universe + 63) / 64;
        BitSet { bits: vec![0u64; words], count: 0 }
    }

    #[inline]
    pub fn insert(&mut self, x: usize) -> bool {
        // Map x to its storage location: word index + bit mask inside that word.
        let (w, b) = (x / 64, 1u64 << (x % 64));
        if self.bits[w] & b == 0 {
            self.bits[w] |= b;
            self.count += 1;
            true
        } else {
            false
        }
    }

    #[inline]
    pub fn remove(&mut self, x: usize) -> bool {
        // Same mapping as insert; clear the bit if it is currently set.
        let (w, b) = (x / 64, 1u64 << (x % 64));
        if self.bits[w] & b != 0 {
            self.bits[w] &= !b;
            self.count -= 1;
            true
        } else {
            false
        }
    }

    #[inline]
    pub fn contains(&self, x: usize) -> bool {
        // Out-of-range words are treated as "not present".
        let w = x / 64;
        w < self.bits.len() && self.bits[w] & (1u64 << (x % 64)) != 0
    }

    pub fn is_empty(&self) -> bool { self.count == 0 }
    pub fn len(&self) -> usize { self.count }

    pub fn iter(&self) -> BitSetIter<'_> {
        BitSetIter {
            bits: &self.bits,
            word_idx: 0,
            word: self.bits.first().copied().unwrap_or(0),
        }
    }

    /// Zero all words and adjust length for a new universe size, reusing allocation.
    pub fn reset(&mut self, universe: usize) {
        let words_needed = (universe + 63) / 64;
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

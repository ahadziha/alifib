/// Dense bitvector for traversal temporaries.
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

    pub fn clone(&self) -> Self {
        BitSet { bits: self.bits.clone(), count: self.count }
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
        while self.word == 0 {
            self.word_idx += 1;
            if self.word_idx >= self.bits.len() { return None; }
            self.word = self.bits[self.word_idx];
        }
        let tz = self.word.trailing_zeros() as usize;
        self.word &= self.word - 1; // clear lowest set bit
        Some(self.word_idx * 64 + tz)
    }
}

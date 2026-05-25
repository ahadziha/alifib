//! Sorted-vector integer sets for per-cell face/coface data.
//!
//! [`IntSet`] is a type alias for `Vec<usize>` kept in sorted, deduplicated
//! order.  Face/coface sets are always small (typically 1–8 elements), so a
//! contiguous sorted vector is faster than any tree- or hash-based alternative:
//! single allocation, no pointer chasing, and O(n+m) merge operations that
//! exploit the sortedness invariant.

/// Sorted `Vec<usize>` for long-lived, per-cell face/coface sets.
/// These are always small (bounded by local cell connectivity, typically 1-8 elements)
/// so a contiguous sorted Vec is cheaper than BTreeSet: one allocation, no pointer chasing.
pub type IntSet = Vec<usize>;

/// Insert x into a sorted, deduplicated Vec, maintaining the invariant.
#[inline]
pub fn insert(v: &mut IntSet, x: usize) {
    match v.binary_search(&x) {
        Ok(_)  => {}
        Err(i) => v.insert(i, x),
    }
}

/// Merge-union of two sorted Vecs.
pub fn union(a: &IntSet, b: &IntSet) -> IntSet {
    use std::cmp::Ordering::*;
    let mut result = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            Less    => { result.push(a[i]); i += 1; }
            Greater => { result.push(b[j]); j += 1; }
            Equal   => { result.push(a[i]); i += 1; j += 1; }
        }
    }
    result.extend_from_slice(&a[i..]);
    result.extend_from_slice(&b[j..]);
    result
}

/// Merge-difference: elements in a that are not in b (both sorted).
pub fn difference(a: &IntSet, b: &IntSet) -> IntSet {
    use std::cmp::Ordering::*;
    let mut result = Vec::with_capacity(a.len());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            Less    => { result.push(a[i]); i += 1; }
            Greater => { j += 1; }
            Equal   => { i += 1; j += 1; }
        }
    }
    result.extend_from_slice(&a[i..]);
    result
}

/// True iff the two sorted Vecs share no element.
pub fn is_disjoint(a: &[usize], b: &[usize]) -> bool {
    use std::cmp::Ordering::*;
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            Less    => i += 1,
            Greater => j += 1,
            Equal   => return false,
        }
    }
    true
}

/// Merge-intersection: elements present in both sorted Vecs.
#[allow(dead_code)]
pub fn intersection(a: &IntSet, b: &IntSet) -> IntSet {
    use std::cmp::Ordering::*;
    let mut result = Vec::with_capacity(a.len().min(b.len()));
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            Less    => i += 1,
            Greater => j += 1,
            Equal   => { result.push(a[i]); i += 1; j += 1; }
        }
    }
    result
}

/// Collect an unsorted iterator into a sorted, deduplicated Vec<usize>.
pub fn collect_sorted(iter: impl Iterator<Item = usize>) -> IntSet {
    let mut v: Vec<usize> = iter.collect();
    v.sort_unstable();
    v.dedup();
    v
}

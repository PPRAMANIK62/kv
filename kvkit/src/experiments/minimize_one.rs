//! Step 1 of the minimizer: remove one item at a time.

/// Returns a part of `input` that still fails: tries removing each item in
/// turn, and keeps the removal whenever the test still fails.
/// `fails(input)` must be true.
pub fn minimize_one<T: Clone>(input: &[T], mut fails: impl FnMut(&[T]) -> bool) -> Vec<T> {
    assert!(fails(input), "the input must fail");
    let mut current = input.to_vec();
    let mut i = 0;
    while i < current.len() {
        let mut candidate = current.clone();
        candidate.remove(i);
        if fails(&candidate) {
            current = candidate;
        } else {
            i += 1;
        }
    }
    current
}

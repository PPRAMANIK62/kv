//! Step 2 of the minimizer: one item at a time, and cut everything after
//! the step that failed.

/// Like `minimize_one`, but `fails` returns `Some(n)` when the test fails
/// using only the first `n` items, and `None` when it passes.
pub fn minimize_cut<T: Clone>(input: &[T], mut fails: impl FnMut(&[T]) -> Option<usize>) -> Vec<T> {
    let n = fails(input).expect("the input must fail");
    let mut current = input[..n].to_vec();
    let mut i = 0;
    while i < current.len() {
        let mut candidate = current.clone();
        candidate.remove(i);
        match fails(&candidate) {
            Some(n) => {
                candidate.truncate(n);
                current = candidate;
            }
            None => i += 1,
        }
    }
    current
}

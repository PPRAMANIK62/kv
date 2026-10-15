//! Shrinking a failing input to a short one that still fails.

/// Returns a short part of `input` that still fails.
///
/// `fails` runs the test on a candidate. It returns `None` if the test
/// passes, and `Some(n)` if it fails using only the first `n` items,
/// so the rest can be cut. `fails(input)` must be `Some`, and `fails` must
/// give the same answer every time for the same candidate.
///
/// The result is 1-minimal: removing any single item from it makes the
/// test pass.
pub fn minimize<T: Clone>(input: &[T], mut fails: impl FnMut(&[T]) -> Option<usize>) -> Vec<T> {
    let Some(n) = fails(input) else {
        panic!("minimize: the input passed when run again; is the test deterministic")
    };
    let mut current = input[..n].to_vec();
    let mut chunk = (current.len() / 2).max(1);
    while chunk > 0 {
        let mut removed_any = false;
        let mut start = 0;
        while start < current.len() {
            // Try the input without current[start..end].
            let end = (start + chunk).min(current.len());
            let mut candidate = current[..start].to_vec();
            candidate.extend_from_slice(&current[end..]);
            match fails(&candidate) {
                Some(n) => {
                    candidate.truncate(n);
                    current = candidate;
                    removed_any = true;
                }
                None => start = end,
            }
        }
        // Halve the chunk only once a whole pass removed nothing,
        // so the last pass is one item at a time and changed nothing.
        if !removed_any {
            chunk /= 2;
        }
    }
    current
}

// Explanation
// CODE READABILITY FINAL BOSS !!!!!!!!!!
//
// Setup: input `[a b c d e f g h]`
// The test fails as soon it has seen both `c` and `f`
// and returns `Some(position just after that point)`
//
// STEP 0 - Confirm the failure, cut the tail
// ------------------------------------------
// input    [a b c d e f g h]
// fails -> Some(6)         (both `c` and `f` seen by item `6`)
// current  [a b c d e f]   (`g` and `h` are dropped)
// chunk  = 6/2 = 3
//
// STEP 1 - pass with chunk = 3
// ----------------------------
// current      [a b c | d e f];  start = 0, end = 0 + 3 = 3, cut [0..3]
// candidate    [d e f]        => None (no `c`)
//                             -> start = 3, end = 3 + 3 = 6, cut [3..6]
// candidate    [a b c]        => None (no `f`)
//                             -> start = 6, end of pass
// nothing removed -> chunk = 3/2 = 1
//
// STEP 2 - pass with chunk = 1
// ----------------------------
// [a b c d e f]    cut `a` -> [b c d e f]  Some(5) ✔ keep, start = 0
// [b c d e f]      cut `b` -> [c d e f]    Some(4) ✔ keep, start = 0
// [c d e f]        cut `c` -> [d e f]      None✘ (no `c`), start = 1
// [c d e f]        cut `d` -> [c e f]      Some(3) ✔ keep, start = 1
// [c e f]          cut `e` -> [c f]        Some(2) ✔ keep, start = 1
// [c f]            cut `f` -> [c]          None✘ (no `f`), start = 2
//                                                          end of pass
// something removed -> chunk stays 1, run another pass
//
// STEP 3 - pass wwith chunk = 1 (check that nothing else can go)
// --------------------------------------------------------------
// [c f]    cut `c` -> [f]  None ✘
// [c f]    cut `f` -> [c]  None ✘
//
// nmothing removed -> chunk = 1/2 = 0 -> STOP
//
// RESULT
// ------
// [c f]    removing either item makes the test PASS (1-minimal)
//
// LOOP
// ----
// How the loop in `minimize` runs:
//
//              ┌─────────────────────────────┐
//              │ chunk = len/2               │
//              └──────────────┬──────────────┘
//                             ▼
//       ┌──────────► pass: start = 0, removed_any = false
//       │                     │
//       │                     ▼
//       │           ┌─────────────────────┐   Some(n)   ┌──────────────────────────┐
//       │           │ cut current[start.. │────────────►│ current = candidate[..n] │
//       │           │  start+chunk]       │             │ removed_any = true       │
//       │           │ does it still fail? │◄────────────│ (start stays, retry here)│
//       │           └─────────┬───────────┘             └──────────────────────────┘
//       │                     │ None
//       │                     ▼
//       │              start += chunk ── more items? ── yes ──► (cut again)
//       │                     │ no
//       │                     ▼
//       │            removed_any? ── yes ──► same chunk, new pass
//       │                     │ no
//       └── chunk > 0 ◄─ chunk /= 2
//                             │ chunk == 0
//                             ▼
//                          return

#[cfg(test)]
mod tests {
    use super::*;

    /// Fails when a 3 comes somewhere before a 7, and says how far it read.
    fn three_then_seven(items: &[u32]) -> Option<usize> {
        let three = items.iter().position(|&x| x == 3)?;
        let seven = items[three..].iter().position(|&x| x == 7)?;
        Some(three + seven + 1)
    }

    #[test]
    fn it_finds_the_two_items_that_matter() {
        let input: Vec<u32> = (0..1000).map(|i| i % 10).collect();
        assert_eq!(minimize(&input, three_then_seven), [3, 7]);
    }

    #[test]
    fn the_result_still_fails_and_is_one_minimal() {
        // Fails when the sum of the first items reaches 100, which leaves
        // many different answers. Check the promise, not one answer.
        let fails = |items: &[u32]| {
            let mut sum = 0;
            for (i, x) in items.iter().enumerate() {
                sum += x;
                if sum >= 100 {
                    return Some(i + 1);
                }
            }
            None
        };
        let input: Vec<u32> = (0..500).map(|i| (i * 7919) % 31).collect();
        let small = minimize(&input, fails);
        assert!(fails(&small).is_some());
        for i in 0..small.len() {
            let mut without = small.clone();
            without.remove(i);
            assert_eq!(fails(&without), None, "{small:?} without item {i}");
        }
    }

    #[test]
    fn it_counts_its_runs() {
        // A dozen runs to find one item among 1,000, not 1,000.
        let input: Vec<u32> = (0..1000).collect();
        let mut runs = 0;
        let small = minimize(&input, |items| {
            runs += 1;
            items.iter().position(|&x| x == 500).map(|i| i + 1)
        });
        assert_eq!(small, [500]);
        assert!(runs < 40, "{runs} runs");
    }

    #[test]
    #[should_panic(expected = "the input passed")]
    fn an_input_that_passes_is_a_mistake() {
        minimize(&[1, 2, 3], |_| None);
    }
}

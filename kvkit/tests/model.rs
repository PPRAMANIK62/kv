//! The model test for `Store`, through its public API only.

use kvkit::{
    Store,
    testkit::{Op, OpGen, check, model_test, scratch_dir},
};

#[test]
fn the_store_matches_the_model() {
    model_test::<Store>("model", &OpGen::default(), 0..20, 1000);
}

/// A longer run: 1,000 seeds of 5,000 steps. Run it with
/// `cargo test --release -p kvkit --test model -- --ignored`.
#[test]
#[ignore = "takes around 47 seconds on my machine"]
fn the_store_matches_the_model_for_longer() {
    model_test::<Store>("model_long", &OpGen::default(), 0..1000, 5000);
}

//////////////////////////////////////////////////////////////////////
// RULE: Record a randomized failure as a regression test before it.
// Our report prints the operations as Rust, so this is a copy-paste.
//////////////////////////////////////////////////////////////////////

#[test]
fn regressions() {
    let dir = scratch_dir("regressions");
    let cases = [
        vec![Op::set(b"", b" \xdbG"), Op::delete(b""), Op::Reopen],
        vec![Op::set(b"", b""), Op::Reopen],
    ];
    for ops in cases {
        if let Err(failure) = check::<Store>(&dir, &ops) {
            panic!("{failure}");
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

//! Experiments behind the minimizer, kept as tests: a planted bug to
//! find, the two simpler minimizers that came before `testkit::minimize`,
//! and a race that compares all three on the same failure.
//!
//! - `off_by_one`: `Planted`, a `Store` whose `replay` stops one record
//!   too early and loses a smallest record at the end of the log.
//! - `minimize_one`: step 1, removes one item at a time.
//! - `minimize_cut`: step 2, also cuts everything after the failing step.
//! - `race`: counts the runs and steps each minimizer needs.
//!
//! Only compiled for tests.

#![cfg(test)]

mod minimize_cut;
mod minimize_one;
mod off_by_one;

mod race;

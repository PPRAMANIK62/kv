//! Experiments behind the minimizer, kept as tests: a planted bug to find.
//!
//! - `off_by_one`: `Planted`, a `Store` whose `replay` stops one record
//!   too early and loses a smallest record at the end of the log.
//!
//! Only compiled for tests.

#![cfg(test)]

mod off_by_one;

//! Tools for testing a store: a seeded random generator, random operation
//! sequences, a model test that compares a store with a map, and a
//! minimizer that shrinks a failing sequence.

mod minimize;
mod model;
mod ops;
mod rng;

pub use minimize::minimize;
pub use model::{Engine, Failure, check, model_test, scratch_dir};
pub use ops::{Op, OpGen};
pub use rng::{Rng, splitmix64};

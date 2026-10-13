//! `kvkit`, the storage library behind `kvctl`.

pub mod codec;
mod store;

pub use store::Store;

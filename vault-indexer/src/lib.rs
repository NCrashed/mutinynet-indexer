#![feature(mpmc_channel)]

pub mod indexer;

pub use indexer::*;

#[cfg(test)]
mod tests;

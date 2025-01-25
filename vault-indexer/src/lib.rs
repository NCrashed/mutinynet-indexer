#![feature(mpmc_channel)]

mod db;
pub mod indexer;

pub use indexer::*;

#[cfg(test)]
mod tests;

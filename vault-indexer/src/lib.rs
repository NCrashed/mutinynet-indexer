#![feature(mpmc_channel)]

mod cache;
mod db;
pub mod indexer;

pub use indexer::*;

#[cfg(test)]
mod tests;

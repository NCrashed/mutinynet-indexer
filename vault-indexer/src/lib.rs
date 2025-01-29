#![feature(mpmc_channel)]

mod cache;
mod db;
pub mod indexer;
mod parser;

pub use indexer::*;

#[cfg(test)]
mod tests;

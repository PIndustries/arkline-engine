//! Ownership and shared-manifold transitions for the fresh independent-bay mode.
//! These helpers do not expose a request handler or enable a world. The caller
//! must resolve the exact authored registration and checked physical routes, then
//! persist these transitions with stock/cargo/ledger in the same transaction.
mod approach;
mod clearance;
mod history;
mod manifold;
mod motion;
mod ownership;
pub(crate) mod world;
pub use approach::*;
pub use clearance::*;
pub use history::*;
pub use manifold::*;
pub use motion::*;
pub use ownership::*;

#[cfg(test)]
mod tests;

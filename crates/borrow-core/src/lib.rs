//! What both sides of borrow need.
//!
//! Anything here is used by the Client and the Agent, so it is defined once. The
//! wire types especially: a message has to mean the same thing on both machines.

pub mod config;
pub mod keys;
pub mod mount;
pub mod preflight;
pub mod protocol;
pub mod stack;
pub mod telemetry;

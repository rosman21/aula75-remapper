//! AULA 75 Control on Linux: the host's entry over the keyboard data source and the baked plan.

/// The baked plan, written by `build.rs`.
const PLAN: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/app.plan"));
/// The compatibility id and its inputs, written by `build.rs` beside the plan.
const COMPAT: &str = include_str!(concat!(env!("OUT_DIR"), "/compat.json"));

include!(concat!(env!("OUT_DIR"), "/entry.rs"));

//! Remap75's data crate: the keyboard protocol, its layout, macros, the USB
//! transport, and (in `source`) the data source the Contract view asks.

pub mod keycodes;
pub mod layout;
pub mod macros;
pub mod protocol;
pub mod remote;
pub mod session;
pub mod sim;
pub mod update;

#[cfg(not(target_arch = "wasm32"))]
pub mod hid;
pub mod service;
pub mod source;

pub use source::Aula;

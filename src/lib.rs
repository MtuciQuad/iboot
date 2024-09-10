#![no_std]

pub mod f411ceu6;
#[cfg(feature = "f411ceu6")]
pub use f411ceu6::*;

pub mod ifrc_iflight_f722_blitz;
#[cfg(feature = "ifrc_iflight_f722_blitz")]
pub use ifrc_iflight_f722_blitz::*;

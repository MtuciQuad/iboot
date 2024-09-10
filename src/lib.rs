#![no_std]

mod f411ceu6;
#[cfg(feature = "f411ceu6")]
pub use f411ceu6::*;

mod ifrc_iflight_f722_blitz;
#[cfg(feature = "ifrc_iflight_f722_blitz")]
pub use ifrc_iflight_f722_blitz::*;

mod gepr_geprc_f722_aio;
#[cfg(feature = "gepr_geprc_f722_aio")]
pub use gepr_geprc_f722_aio::*;

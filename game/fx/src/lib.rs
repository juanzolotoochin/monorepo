//! Deterministic fixed-point math for the game simulation.
//!
//! [`Fx`] is a signed 32.32 fixed-point number stored in an `i64`. Every
//! operation is integer arithmetic, including sqrt, trig and decimal parsing,
//! so results are bit-identical on every platform and compiler. The simulation
//! never uses floating point; `to_f32`/`to_f64` exist only for the view.

mod fx;
pub mod golden;
mod parse;
mod trig;
mod vec;

pub use fx::Fx;
pub use parse::ParseFxError;
pub use vec::{FxVec2, FxVec3};

#[cfg(test)]
mod tests;

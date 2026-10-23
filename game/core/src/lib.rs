//! Renderer-independent, fixed-step agent logistics, production, transport and defense.
mod model;
pub mod navigation;
mod schema;
pub use model::*;
pub use schema::*;
pub type Result<T> = std::result::Result<T, String>;

#[cfg(test)]
mod tests;

//! Windfall Script v1. Bounded arithmetic/statement VM, with no IO or host handles.
//! Compile and run on a control worker. Only `NoteTransform::evaluate` is realtime safe.
mod controller;
mod library;
mod piano;
mod vm;
pub use controller::*;
pub use library::*;
pub use piano::*;
pub use vm::*;

#[cfg(test)]
mod tests;

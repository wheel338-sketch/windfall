//! Windfall Script v1. Bounded arithmetic/statement VM, with no IO or host handles.
//! Compile and run on a control worker. Only `NoteTransform::evaluate` is realtime safe.
mod vm;
mod library;
mod piano;
pub use vm::*;
pub use library::*;
pub use piano::*;

#[cfg(test)]
mod tests;

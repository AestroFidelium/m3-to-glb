//! Library crate for `m3-to-glb`.
//!
//! The conversion pipeline lives here as public modules so that integration
//! tests and `bolero` fuzz targets under `tests/` can drive the parser and
//! converter directly. The binary (`src/main.rs`) is a thin CLI shim over
//! these modules.

pub mod assets;
pub mod cli;
pub mod glb;
pub mod m3;
pub mod processor;
pub mod quat;

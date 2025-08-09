pub mod keypress;
pub mod puppet;
pub mod vm;
pub mod ocr;
mod models;

pub use puppet::PuppetManager;
pub use vm::VmManager;
pub use ocr::OcrEngine;
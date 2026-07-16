//! yfs — local virtual file store (`header.bin` + `data.bin`).

pub mod commands;
mod error;
mod format;
mod name;
mod store;

pub use commands::{FileInfo, YfsState};
pub use error::{Error, Result};
pub use format::{
    DEFAULT_ENTRY_COUNT, FILE_TYPE_BLOB, FILE_TYPE_MARKDOWN, FILE_TYPE_TEXT, MAX_FILE_SIZE,
};
pub use store::{Meta, Store};

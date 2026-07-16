use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub enum Error {
    NotFound,
    AlreadyExists,
    InvalidName(String),
    FileTooLarge,
    StoreFull,
    Corrupt(String),
    Locked(PathBuf),
    Io(io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "not found"),
            Self::AlreadyExists => write!(f, "already exists"),
            Self::InvalidName(msg) => write!(f, "invalid name: {msg}"),
            Self::FileTooLarge => write!(f, "file too large"),
            Self::StoreFull => write!(f, "store full"),
            Self::Corrupt(msg) => write!(f, "corrupt: {msg}"),
            Self::Locked(path) => write!(f, "store locked: {}", path.display()),
            Self::Io(err) => write!(f, "io: {err}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

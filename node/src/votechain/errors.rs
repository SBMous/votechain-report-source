use std::{fmt::Display, io};

#[derive(Debug)]
pub enum Error {
    Heed(heed::Error),
    Io(io::Error),
    BlockNotFound(u32),
    InvalidNewBlock,
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Heed(error) => write!(f, "{}", error),
            Error::Io(error) => write!(f, "{}", error),
            Error::BlockNotFound(index) => write!(f, "No block found at index {}", index),
            Error::InvalidNewBlock => write!(f, "Provided block failed to validate"),
        }
    }
}

impl std::error::Error for Error {}

impl From<heed::Error> for Error {
    fn from(error: heed::Error) -> Error {
        Error::Heed(error)
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Error {
        Error::Io(error)
    }
}
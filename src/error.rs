use crate::http;
use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum AppError {
    AccessTokenNotFoundError(),
    IOError(std::io::Error),
    TomlParseError(toml::de::Error),
    GitError(String),
    HttpError(http::HttpError),
}

impl Error for AppError {
    fn description(&self) -> &str {
        match *self {
            AppError::AccessTokenNotFoundError(..) => "access token not found",
            AppError::IOError(..) => "io error",
            AppError::TomlParseError(..) => "toml parse error",
            AppError::GitError(..) => "git error",
            AppError::HttpError(..) => "http error",
        }
    }
    fn cause(&self) -> Option<&dyn Error> {
        match *self {
            AppError::AccessTokenNotFoundError(..) => None,
            AppError::GitError(..) => None,
            AppError::IOError(ref e) => Some(e),
            AppError::TomlParseError(ref e) => Some(e),
            AppError::HttpError(ref e) => Some(e),
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}: ", self.description())?;
        match *self {
            AppError::AccessTokenNotFoundError(..) => write!(f, "access token not found"),
            AppError::GitError(ref v) => write!(f, "git error: {}", v),
            AppError::IOError(ref e) => write!(f, "{}", e),
            AppError::TomlParseError(ref e) => write!(f, "{}", e),
            AppError::HttpError(ref e) => write!(f, "{}", e),
        }
    }
}
impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::IOError(e)
    }
}

impl From<toml::de::Error> for AppError {
    fn from(e: toml::de::Error) -> Self {
        AppError::TomlParseError(e)
    }
}

impl From<http::HttpError> for AppError {
    fn from(e: http::HttpError) -> Self {
        AppError::HttpError(e)
    }
}

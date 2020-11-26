use std::fmt;

use nom::error::ParseError;
use nom::lib::std::fmt::Formatter;

#[derive(Debug, PartialEq)]
pub enum M3U8ParserError<I> {
    NomError(I, nom::error::ErrorKind),
    IoError(String),
    ParseFloatError(String),
    ParseIntError(String),
}

impl<I: fmt::Display> fmt::Display for M3U8ParserError<I> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            M3U8ParserError::NomError(input, kind) => {
                write!(f, "{}", nom::error::Error::from_error_kind(input, *kind))
            }
            M3U8ParserError::IoError(e) => write!(f, "IO Error: {}", e),
            M3U8ParserError::ParseFloatError(e) => write!(f, "ParseFloat Error: {}", e),
            M3U8ParserError::ParseIntError(e) => write!(f, "ParseInt Error: {}", e),
        }
    }
}

impl<I: fmt::Debug + fmt::Display> std::error::Error for M3U8ParserError<I> {}

impl<I> nom::error::ParseError<I> for M3U8ParserError<I> {
    fn from_error_kind(input: I, kind: nom::error::ErrorKind) -> Self {
        M3U8ParserError::NomError(input, kind)
    }

    fn append(_: I, _: nom::error::ErrorKind, other: Self) -> Self {
        other
    }
}

impl<I> From<nom::error::Error<I>> for M3U8ParserError<I> {
    fn from(err: nom::error::Error<I>) -> Self {
        Self::from_error_kind(err.input, err.code)
    }
}

impl<I> From<std::io::Error> for M3U8ParserError<I> {
    fn from(err: std::io::Error) -> Self {
        M3U8ParserError::IoError(err.to_string())
    }
}

impl<I> From<std::num::ParseIntError> for M3U8ParserError<I> {
    fn from(err: std::num::ParseIntError) -> Self {
        M3U8ParserError::ParseIntError(err.to_string())
    }
}

impl<I> From<std::num::ParseFloatError> for M3U8ParserError<I> {
    fn from(err: std::num::ParseFloatError) -> Self {
        M3U8ParserError::ParseFloatError(err.to_string())
    }
}

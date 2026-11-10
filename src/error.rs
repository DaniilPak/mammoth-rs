//! Error and message types.

use std::fmt;

/// A fatal error that prevented a document from being converted at all.
///
/// Problems that only affect part of a document (an unrecognised style, an
/// image that could not be read, …) are reported as [`Message`]s instead.
#[derive(Debug)]
pub enum Error {
    /// Reading the input (or an external file) failed.
    Io(std::io::Error),
    /// The input is not a readable zip archive.
    Zip(String),
    /// An XML part inside the document could not be parsed.
    Xml {
        /// Path of the part inside the zip archive.
        part: String,
        /// Description of the parse error.
        message: String,
    },
    /// The zip archive does not look like a `.docx` document.
    InvalidDocument(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(error) => write!(f, "{error}"),
            Error::Zip(message) => write!(f, "could not read zip file: {message}"),
            Error::Xml { part, message } => write!(f, "could not parse {part}: {message}"),
            Error::InvalidDocument(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Error::Io(error)
    }
}

impl From<zip::result::ZipError> for Error {
    fn from(error: zip::result::ZipError) -> Self {
        Error::Zip(error.to_string())
    }
}

/// Convenience alias for results returned by this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Severity of a [`Message`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageKind {
    /// Something in the document was ignored or could not be represented.
    Warning,
    /// Part of the document could not be converted (for instance an image).
    Error,
}

impl fmt::Display for MessageKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MessageKind::Warning => f.write_str("warning"),
            MessageKind::Error => f.write_str("error"),
        }
    }
}

/// A non-fatal warning or error produced while converting a document.
///
/// Mirrors the `messages` array returned by mammoth.js.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Message {
    /// Whether this is a warning or an error.
    pub kind: MessageKind,
    /// Human readable description.
    pub message: String,
}

impl Message {
    /// Creates a warning message.
    pub fn warning(message: impl Into<String>) -> Self {
        Message {
            kind: MessageKind::Warning,
            message: message.into(),
        }
    }

    /// Creates an error message.
    pub fn error(message: impl Into<String>) -> Self {
        Message {
            kind: MessageKind::Error,
            message: message.into(),
        }
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

/// Removes duplicate messages while keeping the first occurrence of each.
pub(crate) fn dedupe_messages(messages: Vec<Message>) -> Vec<Message> {
    let mut result: Vec<Message> = Vec::with_capacity(messages.len());
    for message in messages {
        if !result.contains(&message) {
            result.push(message);
        }
    }
    result
}

/// The output of a conversion: the converted text plus any messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionResult {
    /// The generated HTML, Markdown or raw text.
    pub value: String,
    /// Warnings and errors collected during conversion.
    pub messages: Vec<Message>,
}

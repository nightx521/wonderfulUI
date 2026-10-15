use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("hex decode error at position {pos}: {msg}")]
    Hex { msg: String, pos: usize },

    #[error("crypto error: {0}")]
    Crypto(String),

    #[error("file is not valid UTF-8: {0}")]
    InputEncoding(#[from] std::str::Utf8Error),

    #[error("utf-8 error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    #[error("decrypted plaintext is not valid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),

    #[error("decrypted JSON: expected top-level object")]
    NotAnObject,

    #[error("no match list found (expected key \"{0}\")")]
    NoMatchList(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl ParseError {
    /// Stable classification for local persistence and user-facing feedback.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(e) => match e.kind() {
                std::io::ErrorKind::NotFound => "file_missing",
                std::io::ErrorKind::PermissionDenied => "access_denied",
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => "file_busy",
                _ if cfg!(windows) && matches!(e.raw_os_error(), Some(32 | 33)) => "file_busy",
                _ => "read_failed",
            },
            Self::Hex { .. } | Self::InputEncoding(_) => "invalid_file",
            Self::Crypto(_) | Self::Utf8(_) => "decrypt_failed",
            Self::InvalidJson(e) if e.is_data() => "unsupported_format",
            Self::InvalidJson(_) => "invalid_json",
            Self::NotAnObject | Self::NoMatchList(_) => "unsupported_format",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_io_and_parse_failures_without_matching_display_text() {
        for (kind, code) in [
            (std::io::ErrorKind::NotFound, "file_missing"),
            (std::io::ErrorKind::PermissionDenied, "access_denied"),
            (std::io::ErrorKind::WouldBlock, "file_busy"),
            (std::io::ErrorKind::TimedOut, "file_busy"),
            (std::io::ErrorKind::Other, "read_failed"),
        ] {
            assert_eq!(ParseError::Io(std::io::Error::from(kind)).code(), code);
        }
        if cfg!(windows) {
            assert_eq!(
                ParseError::Io(std::io::Error::from_raw_os_error(32)).code(),
                "file_busy"
            );
            assert_eq!(
                ParseError::Io(std::io::Error::from_raw_os_error(33)).code(),
                "file_busy"
            );
        }
        assert_eq!(
            ParseError::Hex {
                msg: "test".into(),
                pos: 0
            }
            .code(),
            "invalid_file"
        );
        assert_eq!(ParseError::Crypto("test".into()).code(), "decrypt_failed");
        assert_eq!(ParseError::NotAnObject.code(), "unsupported_format");
        let syntax = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        assert_eq!(ParseError::InvalidJson(syntax).code(), "invalid_json");
        let data = serde_json::from_str::<Vec<String>>("{}").unwrap_err();
        assert_eq!(ParseError::InvalidJson(data).code(), "unsupported_format");
    }
}

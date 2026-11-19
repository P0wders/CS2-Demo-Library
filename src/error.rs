use std::fmt;

/// Errors produced while opening or parsing a demo.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("not a CS2 demo file (magic was {0:?}, expected \"PBDEMS2\\0\")")]
    BadMagic([u8; 8]),

    #[error("unexpected end of data while reading {0}")]
    UnexpectedEof(&'static str),

    #[error("snappy decompression failed: {0}")]
    Snappy(#[from] snap::Error),

    #[error("failed to decode protobuf {what}: {source}")]
    Proto {
        what: &'static str,
        #[source]
        source: prost::DecodeError,
    },

    #[error("malformed demo: {0}")]
    Malformed(String),
}

impl Error {
    pub(crate) fn malformed(msg: impl fmt::Display) -> Self {
        Error::Malformed(msg.to_string())
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Decodes a protobuf message, tagging failures with the message name.
pub(crate) fn decode_proto<M: prost::Message + Default>(what: &'static str, data: &[u8]) -> Result<M> {
    M::decode(data).map_err(|source| Error::Proto { what, source })
}

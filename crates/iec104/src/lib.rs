//! A small, dependency-free implementation of the IEC 60870-5-104 telecontrol
//! protocol: APDU framing, the ASDU types a DSO needs to control a prosumer
//! site, and the link layer of a controlled station (server) as a sans-IO
//! state machine. The optional `tokio` feature adds a connection driver.

pub mod apci;
pub mod asdu;
pub mod describe;
pub mod session;
pub mod time;

#[cfg(feature = "tokio")]
pub mod connection;

pub use apci::{Apdu, UFunction};
pub use asdu::{Asdu, AsduError, Cause, Element, InformationObject, Quality};
pub use session::{Close, Config, Output, Session};
pub use time::Cp56Time2a;

/// Framing errors. Any of them ends the connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Truncated,
    BadStart(u8),
    BadLength(usize),
    BadControlField(u8),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Truncated => write!(f, "truncated frame"),
            Error::BadStart(b) => write!(f, "expected start octet 0x68, got {b:#04x}"),
            Error::BadLength(l) => write!(f, "invalid APDU length {l}"),
            Error::BadControlField(b) => write!(f, "invalid control field {b:#04x}"),
        }
    }
}

impl std::error::Error for Error {}

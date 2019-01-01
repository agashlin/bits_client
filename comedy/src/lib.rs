extern crate failure;
extern crate failure_derive;
extern crate winapi;
extern crate wio;

#[cfg(feature = "guid_serde")]
extern crate serde;
#[cfg(feature = "guid_serde")]
extern crate serde_derive;

pub mod bstr;
pub mod com;
pub mod error;
pub mod guid;
pub mod handle;
pub mod process;
pub mod variant;

pub use error::Error;

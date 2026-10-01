mod device;
pub mod evdev;
#[cfg(feature = "host")]
mod host;
#[cfg(feature = "host")]
mod keys;
mod mask;
#[cfg(feature = "host")]
mod pad;
pub mod trace;

pub use device::{faster_poll, DeviceInput, POLL_MS};
#[cfg(feature = "host")]
pub use host::HostInput;
pub use mask::Pad;

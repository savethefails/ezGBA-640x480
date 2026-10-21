mod alsa;
#[cfg(feature = "host")]
mod host;
mod pacer;
mod ring;
mod sfx;
mod silence;
mod sink;
mod stub;
pub mod volume;

pub use alsa::AlsaSink;
#[cfg(feature = "host")]
pub use host::HostAudio;
pub use pacer::Pacer;
pub use ring::{ring_capacity, Ring};
pub use sfx::{render_pcm, Sfx};
pub use silence::Silence;
pub use sink::{AudioError, AudioSink};
pub use stub::StubSink;

/// The GBA's own rate. slot plays nothing else, so the device is opened for it before there
/// is a core to ask, and a device that takes it needs no resampling at all.
pub const GBA_HZ: u32 = 32_768;

/// The sink this build talks to: cpal on a desktop, ALSA on the device. A sink that fails to
/// open is not a failure to boot either way, since a ring nothing drains still lets the
/// emulator run.
#[cfg(feature = "host")]
pub fn open_sink() -> Box<dyn AudioSink> {
    Box::new(HostAudio::new())
}

/// `open_sink`, opened at `GBA_HZ`. A frontend for one console knows the rate before it knows
/// the cart. A device that refuses it still opens, and the worker resamples to whatever it did
/// take. On its own so the boot can open it on a thread.
pub fn opened_sink() -> Box<dyn AudioSink> {
    let mut sink = open_sink();
    if let Err(e) = sink.open(GBA_HZ) {
        eprintln!("slot: audio: {e}");
    }
    sink
}

#[cfg(not(feature = "host"))]
pub fn open_sink() -> Box<dyn AudioSink> {
    Box::new(AlsaSink::new())
}

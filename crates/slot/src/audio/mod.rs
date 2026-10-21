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

/// The rate the device is opened at, before there is a core to ask: 48 kHz, which the SP's codec
/// plays natively, and which every core's sound is resampled to through a windowed sinc. It was
/// the GBA's own 32768 Hz, which mGBA's 65536 Hz output had to be halved to. From slot 5dc5b83.
pub const OUTPUT_HZ: u32 = 48_000;

/// The sink this build talks to: cpal on a desktop, ALSA on the device. A sink that fails to
/// open is not a failure to boot either way, since a ring nothing drains still lets the
/// emulator run.
#[cfg(feature = "host")]
pub fn open_sink() -> Box<dyn AudioSink> {
    Box::new(HostAudio::new())
}

/// `open_sink`, opened at `OUTPUT_HZ`. A frontend for one console knows the rate before it knows
/// the cart. A device that refuses it still opens, and the worker resamples to whatever it did
/// take. On its own so the boot can open it on a thread.
pub fn opened_sink() -> Box<dyn AudioSink> {
    let mut sink = open_sink();
    if let Err(e) = sink.open(OUTPUT_HZ) {
        eprintln!("slot: audio: {e}");
    }
    sink
}

#[cfg(not(feature = "host"))]
pub fn open_sink() -> Box<dyn AudioSink> {
    Box::new(AlsaSink::new())
}

//! Playback engine: transport, device supervision and the bridge between
//! decoders, DSP and the OS audio stack.
//!
//! # Thread topology
//!
//! ```text
//!  IPC thread                 supervisor thread              device thread
//!  ──────────                 ─────────────────              ─────────────
//!  play/pause/seek  ───────►  GaplessPlayer
//!                             DspChain
//!                                  │ f32 frames
//!                                  ▼
//!                            SampleProducer ─── ring ───► SampleConsumer
//!                                  ▲                            │
//!                                  └──── RefillSignal ◄─────────┘
//! ```
//!
//! Only the supervisor touches the decoder and the DSP chain; only the device
//! thread touches the DAC. They meet in a lock-free SPSC ring, so a slow
//! decoder can never stall the audio callback and a busy device can never
//! block the IPC layer.

pub mod backend;
mod supervisor;

#[cfg(target_os = "linux")]
mod alsa_out;
#[cfg(target_os = "linux")]
mod pipewire_out;

mod cpal_out;

#[cfg(windows)]
mod wasapi_excl;

pub use backend::{
    BackendKind, Negotiation, OpenedSink, OutputDevice, OutputMode, OutputSink, RefillSignal,
    SinkDescriptor,
};
pub use supervisor::{
    Engine, EngineConfig, EngineEvent, EngineStatus, NowPlaying, PlaybackState, SinkOpener,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_have_stable_identifiers() {
        assert_eq!(OutputMode::Auto.id(), "auto");
        assert_eq!(OutputMode::Exclusive.id(), "exclusive");
        assert_eq!(OutputMode::Shared.id(), "shared");
        assert_eq!(OutputMode::Asio.id(), "asio");
        assert_eq!(OutputMode::default(), OutputMode::Auto);
    }

    #[test]
    fn auto_mode_starts_with_an_exclusive_endpoint() {
        let candidates = backend::preferred_kinds(OutputMode::Auto);
        assert!(
            candidates.first().is_some_and(|kind| kind.is_exclusive()),
            "Auto must try to own the device before falling back to mixing"
        );
    }

    #[test]
    fn every_mode_falls_back_to_the_system_mixer() {
        for mode in [OutputMode::Auto, OutputMode::Exclusive, OutputMode::Shared] {
            let candidates = backend::preferred_kinds(mode);
            assert!(
                candidates.contains(&BackendKind::CpalShared),
                "{mode:?} would leave the user without sound when the preferred stack is missing"
            );
        }
    }
}

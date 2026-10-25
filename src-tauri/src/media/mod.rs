//! Native operating-system media integration.

#[cfg(windows)]
pub mod windows;

/// Internal renderer command used for queue-aware SMTC next/previous buttons.
/// Payloads are the literal strings `next` and `previous` and are parsed by the
/// same strict IPC boundary as every other backend event.
pub const MEDIA_COMMAND_CHANNEL: &str = "media://command";

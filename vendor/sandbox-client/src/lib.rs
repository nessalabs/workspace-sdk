#![doc = include_str!("../docs/guide.md")]

mod client;
mod config;
mod error;
pub mod handoff;
mod helpers;
pub mod service;
mod stream;
mod types;
mod wait;
pub use stream::{stream_events, stream_prompt, StreamEventsOptions, StreamPromptOptions};
pub use tokio_util::sync::CancellationToken;

pub use client::Api;
/// Branded entry point; internal transport uses the stable `Api` type.
pub use client::Api as BoatApi;
/// Branded configuration alias; defaults live in `service`.
pub use config::ClientConfig as BoatClientConfig;
pub use config::{ClientConfig, RetryConfig};
pub use error::{Error, Result};
pub use helpers::{exec_command, read_text, stop_and_remove, stop_and_remove_with, write_text};
pub use types::*;
pub use wait::{
    wait_for_deletion, wait_for_desktop, wait_for_desktop_with, wait_for_prompt, wait_until_idle,
    wait_until_idle_with, wait_until_ready, wait_until_ready_with, DesktopWaitOptions, WaitOptions,
};

//! BOAT_API_KEY=… SANDBOX_ID=bx_… BOX_PROMPT='…' cargo run --example stream_prompt
//! Queues one prompt and prints event IDs/types until that prompt finishes.
use boat_client::{stream_prompt, BoatApi, BoatClientConfig, Error, PromptProvider, PromptRequest};
use futures_util::{pin_mut, StreamExt};

#[tokio::main]
async fn main() -> boat_client::Result<()> {
    let api = BoatApi::new(BoatClientConfig::from_env()?)?;
    let sandbox_id =
        std::env::var("SANDBOX_ID").map_err(|_| Error::Config("set SANDBOX_ID".into()))?;
    let prompt = std::env::var("BOX_PROMPT").map_err(|_| Error::Config("set BOX_PROMPT".into()))?;
    let stream = stream_prompt(
        &api,
        &sandbox_id,
        PromptRequest::new(PromptProvider::Codex, prompt),
        None,
    );
    pin_mut!(stream);
    while let Some(event) = stream.next().await {
        let event = event?;
        println!(
            "{} {}",
            event.id.as_deref().unwrap_or("(no id)"),
            event.type_
        );
    }
    Ok(())
}

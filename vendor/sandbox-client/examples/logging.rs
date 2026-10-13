//! RUST_LOG=boat_client=debug cargo run --example logging
use boat_client::{BoatApi, BoatClientConfig, Result};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();
    let api = BoatApi::new(BoatClientConfig::from_env()?)?;
    let response = api.sandboxes(None).await?;
    println!("{} sandboxes", response.sandboxes.len());
    Ok(())
}

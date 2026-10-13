//! Smoke: get + exec against an existing sandbox.
//!
//! ```bash
//! BOAT_API_KEY=… SANDBOX_ID=bx_… cargo run --example exec_smoke
//! ```
use boat_client::{wait_until_ready, BoatApi, BoatClientConfig, CommandRequest, ResumeRequest};

#[tokio::main]
async fn main() -> boat_client::Result<()> {
    let api = BoatApi::new(BoatClientConfig::from_env()?)?;
    let sandbox_id = std::env::var("SANDBOX_ID").map_err(|_| {
        boat_client::Error::Config("set SANDBOX_ID to a box id (bx_…); no default".into())
    })?;

    let info = api.get(&sandbox_id).await?;
    println!("state={}", info.sandbox.state.as_str());
    if !info.sandbox.state.is_operable() {
        println!("resuming…");
        api.resume(&sandbox_id, Some(ResumeRequest::default()))
            .await?;
        wait_until_ready(&api, &sandbox_id).await?;
    }

    let out = api
        .command(
            &sandbox_id,
            CommandRequest::new("uname -a && command -v opencode").with_timeout_seconds(60),
        )
        .await?;
    println!("exit={:?} timed_out={}", out.exit_code, out.timed_out);
    print!("{}", out.stdout);
    if !out.stderr.is_empty() {
        eprint!("{}", out.stderr);
    }
    Ok(())
}

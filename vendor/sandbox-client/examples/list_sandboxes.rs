use boat_client::{BoatApi, BoatClientConfig};

#[tokio::main]
async fn main() -> boat_client::Result<()> {
    let api = BoatApi::new(BoatClientConfig::from_env()?)?;
    let list = api.sandboxes(None).await?;
    for b in &list.sandboxes {
        println!(
            "{}  state={:<12}  name={}",
            b.id,
            b.state.as_str(),
            b.name.as_deref().unwrap_or("<unnamed>")
        );
    }
    println!("{} box(es)", list.sandboxes.len());
    Ok(())
}

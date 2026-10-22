#[tokio::main]
async fn main() -> anyhow::Result<()> {
    broker_login::app::run().await
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    prompt_ferry::app::run_cli().await
}

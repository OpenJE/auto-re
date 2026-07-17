use auto_re::Result;

#[tokio::main]
async fn main() -> Result<()> {
    auto_re::cli::run().await
}

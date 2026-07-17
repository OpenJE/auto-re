#[tokio::main]
async fn main() -> autore_stage1::Result<()> {
    autore_stage1::cli::run().await
}

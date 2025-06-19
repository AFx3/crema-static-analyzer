mod github;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {

    let repos = github::search_rust_main_with_unsafe_ops().await?;

    println!("\nFound repos:");
    for r in repos {
        println!("- {}", r);
    }
    Ok(())
}

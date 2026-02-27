mod brain;
mod executor;

use brain::{Brain, BrainConfig, RequestBuilder};
use tracing_subscriber::fmt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    fmt().with_max_level(tracing::Level::INFO).init();

    // Load configuration from environment
    let config = BrainConfig::from_env()?;

    println!("Initializing Brain with model: {}", config.default_model);

    // Create Brain instance
    let brain = Brain::new(config).await?;

    println!("Brain initialized successfully!");

    // Example inference
    let request = RequestBuilder::new(brain.default_model().to_string())
        .system("You are a helpful assistant.")
        .user_text("Hello, what is 2 + 2?")
        .max_tokens(brain.max_output_tokens())
        .build()?;

    match brain.infer(request).await {
        Ok(response) => {
            println!("\n=== Response ===");
            println!("Model: {}", response.model);
            println!("Stop reason: {:?}", response.stop_reason);
            if let Some(usage) = &response.usage {
                println!(
                    "Usage: input={}, output={}",
                    usage.input_tokens, usage.output_tokens
                );
            }

            for block in &response.content {
                if let brain::ContentBlock::Text { text } = block {
                    println!("\nText: {}", text);
                }
            }
        }
        Err(e) => {
            eprintln!("Inference error: {}", e);
        }
    }

    Ok(())
}

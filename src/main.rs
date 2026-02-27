mod brain;
mod comm;
mod executor;

use brain::{Brain, BrainConfig, RequestBuilder};
use comm::{Comm, CommConfig};
use tracing_subscriber::fmt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    fmt().with_max_level(tracing::Level::INFO).init();

    // Initialize comm
    let config = CommConfig::default();
    println!("Comm initialized, listening on 0.0.0.0:{}", config.listen_port);

    let (comm, mut user_rx) = Comm::new(config).await?;

    // Initialize brain
    let brain_config = BrainConfig::from_env()?;
    println!("Initializing Brain with model: {}", brain_config.default_model);
    let brain = Brain::new(brain_config).await?;
    println!("Brain initialized successfully!");

    // Spawn comm server
    tokio::spawn(async move {
        if let Err(e) = comm.run().await {
            eprintln!("Comm error: {}", e);
        }
    });

    // Main loop: handle user requests
    loop {
        tokio::select! {
            Some(req) = user_rx.recv() => {
                println!("Received request from {}: {}", req.source_addr, req.content);

                // Process with brain
                let request = RequestBuilder::new(brain.default_model().to_string())
                    .system("You are a helpful assistant that responds to user commands.")
                    .user_text(&req.content)
                    .max_tokens(brain.max_output_tokens())
                    .build();

                let response = match request {
                    Ok(req) => {
                        match brain.infer(req).await {
                            Ok(resp) => {
                                let content = resp.content.iter()
                                    .filter_map(|block| {
                                        if let brain::ContentBlock::Text { text } = block {
                                            Some(text.clone())
                                        } else {
                                            None
                                        }
                                    })
                                    .collect::<Vec<_>>()
                                    .join("");
                                comm::UserResponse::new(content)
                            }
                            Err(e) => {
                                comm::UserResponse::error(format!("Brain error: {}", e))
                            }
                        }
                    }
                    Err(e) => {
                        comm::UserResponse::error(format!("Request build error: {}", e))
                    }
                };

                // Send response back to comm
                if req.reply.send(response).is_err() {
                    eprintln!("Failed to send response");
                }
            }
        }
    }
}

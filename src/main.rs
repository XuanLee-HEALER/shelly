mod agent;
mod brain;
mod comm;
mod executor;
mod memory;

use agent::{AgentConfig, AgentLoop};
use brain::Brain;
use brain::BrainConfig;
use comm::{Comm, CommConfig};
use executor::{Executor, ExecutorConfig};
use std::process;
use tokio::signal;
use tracing::{error, info, Level};
use tracing_subscriber::fmt;

/// Tokio runtime with signal handling
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging with high observability for dev
    fmt()
        .with_max_level(Level::DEBUG)
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .init();

    info!("Starting Shelly daemon...");

    // Initialize config
    let comm_config = CommConfig::default();
    let brain_config = BrainConfig::from_env()?;
    let executor_config = ExecutorConfig::default();
    let agent_config = AgentConfig::from_env()?;

    info!(
        comm_port = comm_config.listen_port,
        model = %brain_config.default_model,
        "Configuration loaded"
    );

    // Initialize comm
    let (comm, mut user_rx) = Comm::new(comm_config).await?;
    info!(addr = %comm.local_addr()?, "Comm initialized");

    // Initialize brain
    let brain = Brain::new(brain_config).await?;
    info!(model = brain.default_model(), "Brain initialized");

    // Initialize executor
    let executor = Executor::new(executor_config);
    info!(tools = executor.tool_definitions().len(), "Executor initialized");

    // Initialize agent loop
    let agent = AgentLoop::new(brain, executor, agent_config);

    // Spawn comm server
    let comm_handle = tokio::spawn(async move {
        if let Err(e) = comm.run().await {
            error!(error = %e, "Comm server error");
        }
    });

    // Run initialization
    info!("Running agent initialization...");
    if let Err(e) = agent.run_init().await {
        error!(error = %e, "Agent initialization failed");
        process::exit(1);
    }

    // Main loop with signal handling
    info!("Entering main loop...");

    loop {
        tokio::select! {
            // Handle user requests
            Some(req) = user_rx.recv() => {
                agent.handle_user_request(req).await;
            }
            // Handle Ctrl+C / SIGTERM
            _ = async {
                signal::ctrl_c().await.ok();
            } => {
                info!("Received shutdown signal");
                break;
            }
        }
    }

    // Shutdown handling
    info!("Starting shutdown...");
    agent.shutdown().await;

    // Clean up
    info!("Shutting down...");
    comm_handle.abort();

    info!("Goodbye!");
    Ok(())
}

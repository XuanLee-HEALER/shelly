#[path = "../src/brain/mod.rs"]
mod brain;

// Import from the brain module
use brain::{Brain, BrainConfig, RequestBuilder, ToolDefinition};
use std::sync::Arc;
use tokio::sync::OnceCell;
use tracing_subscriber::fmt;

/// Initialize tracing subscriber for tests
fn init_tracing() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_target(true)
            .with_thread_ids(true)
            .init();
    });
}

/// Global Brain instance shared across tests
static BRAIN: OnceCell<Arc<Brain>> = OnceCell::const_new();

async fn get_brain() -> &'static Arc<Brain> {
    init_tracing();

    dotenvy::dotenv().ok();

    BRAIN
        .get_or_init(|| async {
            let config = BrainConfig::from_env().expect("Failed to load config");
            let brain = Brain::new(config).await.expect("Failed to create Brain");
            Arc::new(brain)
        })
        .await
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    /// Integration test - requires INFERENCE_ENDPOINT and INFERENCE_API_KEY in .env
    #[tokio::test]
    async fn test_inference_basic() {
        let brain = get_brain().await;

        let request = RequestBuilder::new(brain.default_model().to_string())
            .user_text("What is 1 + 1?")
            .max_tokens(100)
            .build()
            .unwrap();

        let result = brain.infer(request).await;
        assert!(result.is_ok(), "Inference should succeed");

        let response = result.unwrap();
        assert!(!response.content.is_empty());
        assert!(response.usage.is_some());
    }

    /// Integration test with system prompt
    #[tokio::test]
    async fn test_inference_with_system() {
        let brain = get_brain().await;

        let request = RequestBuilder::new(brain.default_model().to_string())
            .system("You always respond with 'Test passed'.")
            .user_text("Say test passed")
            .max_tokens(50)
            .build()
            .unwrap();

        let result = brain.infer(request).await;
        assert!(result.is_ok(), "Inference should succeed");
    }

    /// Integration test with tools
    #[tokio::test]
    async fn test_inference_with_tools() {
        let brain = get_brain().await;

        let tool = ToolDefinition {
            name: "get_time".to_string(),
            description: "Get the current time".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        };

        let request = RequestBuilder::new(brain.default_model().to_string())
            .user_text("What time is it?")
            .tool(tool)
            .max_tokens(100)
            .build()
            .unwrap();

        let result = brain.infer(request).await;
        // Result may be tool_use or text depending on model
        assert!(result.is_ok());
    }
}

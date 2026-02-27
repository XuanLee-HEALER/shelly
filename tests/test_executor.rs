// Integration tests for Executor module
// This file should be run with cargo test --test test_executor

#[path = "../src/brain/mod.rs"]
mod brain;

#[path = "../src/executor/mod.rs"]
mod executor;

fn init_tracing() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_target(true)
            .with_thread_ids(true)
            .init();
    });
}

fn create_executor() -> executor::Executor {
    let config = executor::ExecutorConfig {
        tools_toml_path: std::path::PathBuf::from("tools.toml"),
        ..Default::default()
    };
    executor::Executor::init(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test basic bash command execution
    #[tokio::test]
    async fn test_bash_echo() {
        init_tracing();

        let executor = create_executor();

        let input = serde_json::json!({
            "command": "echo hello"
        });

        let result = executor.execute("bash", input).await;
        assert!(result.is_ok(), "Execution should succeed");

        let output = result.unwrap();
        assert!(
            output.content.contains("hello"),
            "Output should contain 'hello'"
        );
        assert!(!output.is_error, "Exit code 0 should not be an error");
    }

    /// Test bash with non-zero exit code
    #[tokio::test]
    async fn test_bash_error_exit() {
        init_tracing();

        let executor = create_executor();

        let input = serde_json::json!({
            "command": "exit 1"
        });

        let result = executor.execute("bash", input).await;
        assert!(result.is_ok(), "Execution should succeed");

        let output = result.unwrap();
        assert!(output.is_error, "Non-zero exit code should be an error");
    }

    /// Test unknown tool
    #[tokio::test]
    async fn test_unknown_tool() {
        init_tracing();

        let executor = create_executor();

        let input = serde_json::json!({
            "command": "echo test"
        });

        let result = executor.execute("nonexistent", input).await;
        assert!(result.is_err(), "Unknown tool should return error");
    }

    /// Test invalid input
    #[tokio::test]
    async fn test_invalid_input() {
        init_tracing();

        let executor = create_executor();

        // Missing required "command" field
        let input = serde_json::json!({
            "wrong_field": "value"
        });

        let result = executor.execute("bash", input).await;
        assert!(result.is_err(), "Invalid input should return error");
    }

    /// Test tool_definitions
    #[tokio::test]
    async fn test_tool_definitions() {
        init_tracing();

        let executor = create_executor();

        let defs = executor.tool_definitions();
        assert!(!defs.is_empty(), "Should have at least one tool");

        let bash_def = defs
            .iter()
            .find(|d| d.name == "bash")
            .expect("Should have bash tool");
        assert!(
            !bash_def.description.is_empty(),
            "Bash should have description"
        );
        assert!(
            bash_def.input_schema.is_object(),
            "Should have input schema"
        );
    }

    /// Test multiline command
    #[tokio::test]
    async fn test_bash_multiline() {
        init_tracing();

        let executor = create_executor();

        let input = serde_json::json!({
            "command": "echo line1 && echo line2"
        });

        let result = executor.execute("bash", input).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.content.contains("line1"));
        assert!(output.content.contains("line2"));
    }
}

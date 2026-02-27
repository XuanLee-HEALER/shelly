// Memory storage and operations

use std::collections::VecDeque;
use std::fs;

use super::config::MemoryConfig;
use super::error::MemoryError;
use super::similarity::cosine_similarity;
use super::types::{JournalEntry, MemoryEntry};
use tracing::{debug, info};

/// Maximum number of journal entries to keep
const MAX_JOURNAL_ENTRIES: usize = 100;

/// Memory - stores agent's semantic memory and journal
#[derive(Debug, Clone, Default)]
pub struct Memory {
    /// Semantic memory entries
    #[allow(dead_code)]
    entries: Vec<MemoryEntry>,
    /// Journal entries (backward compatible)
    journal: VecDeque<JournalEntry>,
    /// Identity (static info about the agent)
    identity: String,
    /// Topology (known system structure)
    topology: Vec<String>,
    /// Configuration
    #[allow(dead_code)]
    config: MemoryConfig,
}

impl Memory {
    /// Create new empty memory with identity (backward compatible)
    pub fn new(identity: String) -> Self {
        Self {
            entries: Vec::new(),
            journal: VecDeque::new(),
            identity,
            topology: Vec::new(),
            config: MemoryConfig::default(),
        }
    }

    /// Load memory from disk
    #[allow(dead_code)]
    pub fn load(config: MemoryConfig) -> Result<Self, MemoryError> {
        let entries_file = config.storage_dir.join("entries.json");

        if !entries_file.exists() {
            info!("Memory file not found, starting with empty memory");
            return Ok(Self {
                entries: Vec::new(),
                journal: VecDeque::new(),
                identity: String::new(),
                topology: Vec::new(),
                config,
            });
        }

        let content = fs::read_to_string(&entries_file)
            .map_err(|e| MemoryError::LoadFailed(e.to_string()))?;

        let entries: Vec<MemoryEntry> =
            serde_json::from_str(&content).map_err(|e| MemoryError::LoadFailed(e.to_string()))?;

        info!("Loaded {} memory entries", entries.len());

        Ok(Self {
            entries,
            journal: VecDeque::new(),
            identity: String::new(),
            topology: Vec::new(),
            config,
        })
    }

    /// Store a memory entry
    #[allow(dead_code)]
    pub async fn store(&mut self, entry: MemoryEntry) -> Result<(), MemoryError> {
        // Ensure storage directory exists
        fs::create_dir_all(&self.config.storage_dir)
            .map_err(|e| MemoryError::StoreFailed(e.to_string()))?;

        // Add entry
        self.entries.push(entry);

        // Persist to disk
        self.persist()?;

        Ok(())
    }

    /// Persist entries to disk
    #[allow(dead_code)]
    fn persist(&self) -> Result<(), MemoryError> {
        let entries_file = self.config.storage_dir.join("entries.json");

        let content = serde_json::to_string_pretty(&self.entries)
            .map_err(|e| MemoryError::StoreFailed(e.to_string()))?;

        fs::write(&entries_file, content).map_err(|e| MemoryError::StoreFailed(e.to_string()))?;

        debug!("Persisted {} memory entries", self.entries.len());

        Ok(())
    }

    /// Recall relevant memories by semantic similarity
    #[allow(dead_code)]
    pub fn recall(&self, _query: &str, query_embedding: &[f32], top_k: usize) -> Vec<MemoryEntry> {
        if self.entries.is_empty() {
            return Vec::new();
        }

        // Calculate similarities
        let mut similarities: Vec<(usize, f32)> = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let sim = cosine_similarity(query_embedding, &entry.embedding);
                (i, sim)
            })
            .collect();

        // Sort by similarity (descending)
        similarities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Return top-k entries
        similarities
            .into_iter()
            .take(top_k)
            .map(|(i, _)| self.entries[i].clone())
            .collect()
    }

    /// Get all entries
    #[allow(dead_code)]
    pub fn entries(&self) -> &[MemoryEntry] {
        &self.entries
    }

    /// Get configuration
    #[allow(dead_code)]
    pub fn config(&self) -> &MemoryConfig {
        &self.config
    }

    /// Generate context string from recalled entries
    #[allow(dead_code)]
    pub fn context_from_recall(&self, entries: &[MemoryEntry]) -> String {
        if entries.is_empty() {
            return String::new();
        }

        let mut parts = vec!["## Relevant Memory".to_string()];

        for entry in entries {
            let time_str = entry.timestamp.format("%Y-%m-%d %H:%M:%S").to_string();
            parts.push(format!("- [{}] {}", time_str, entry.content));
        }

        parts.join("\n")
    }

    // =====================
    // Backward compatible methods
    // =====================

    /// Add entry to journal
    pub fn add(&mut self, entry: JournalEntry) {
        self.journal.push_back(entry);
        // Trim if too large
        while self.journal.len() > MAX_JOURNAL_ENTRIES {
            self.journal.pop_front();
        }
    }

    /// Add system info
    #[allow(dead_code)]
    pub fn add_system_info(&mut self, info: impl Into<String>) {
        self.add(JournalEntry::SystemInfo(info.into()));
    }

    /// Add user interaction
    pub fn add_interaction(&mut self, query: impl Into<String>, response: impl Into<String>) {
        self.add(JournalEntry::UserInteraction {
            query: query.into(),
            response: response.into(),
        });
    }

    /// Add tool result
    pub fn add_tool_result(&mut self, tool: impl Into<String>, result: impl Into<String>) {
        self.add(JournalEntry::ToolResult {
            tool: tool.into(),
            result: result.into(),
        });
    }

    /// Add observation
    pub fn add_observation(&mut self, observation: impl Into<String>) {
        self.add(JournalEntry::Observation(observation.into()));
    }

    /// Add error
    pub fn add_error(&mut self, error: impl Into<String>) {
        self.add(JournalEntry::Error(error.into()));
    }

    /// Add topology info
    #[allow(dead_code)]
    pub fn add_topology(&mut self, info: impl Into<String>) {
        self.topology.push(info.into());
    }

    /// Generate context string for system prompt
    pub fn context(&self) -> String {
        let mut parts = Vec::new();

        // Identity
        if !self.identity.is_empty() {
            parts.push(format!("## Identity\n{}", self.identity));
        }

        // Topology
        if !self.topology.is_empty() {
            parts.push(format!("## Known Topology\n{}", self.topology.join("\n")));
        }

        // Recent journal (last 10 entries)
        let recent: Vec<_> = self.journal.iter().rev().take(10).collect();
        if !recent.is_empty() {
            let journal_str = recent
                .iter()
                .rev()
                .map(|e| format!("- {}", e))
                .collect::<Vec<_>>()
                .join("\n");
            parts.push(format!("## Recent History\n{}", journal_str));
        }

        parts.join("\n\n")
    }

    /// Get full journal for debugging
    #[allow(dead_code)]
    pub fn journal_entries(&self) -> Vec<&JournalEntry> {
        self.journal.iter().collect()
    }

    /// Set identity
    #[allow(dead_code)]
    pub fn set_identity(&mut self, identity: impl Into<String>) {
        self.identity = identity.into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_entry_creation() {
        let entry = MemoryEntry::new("Test content".to_string(), vec![0.1, 0.2, 0.3]);
        assert!(!entry.id.is_empty());
        assert_eq!(entry.content, "Test content");
        assert_eq!(entry.embedding, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn test_memory_empty_recall() {
        let memory = Memory::default();
        let results = memory.recall("query", &[0.1, 0.2, 0.3], 5);
        assert!(results.is_empty());
    }

    #[test]
    fn test_memory_context() {
        let mut memory = Memory::new("Shelly".to_string());
        memory.add_observation("Test observation");
        memory.add_tool_result("test_tool", "result");

        let ctx = memory.context();
        assert!(ctx.contains("Shelly"));
        assert!(ctx.contains("observation"));
    }

    #[test]
    fn test_memory_backward_compatible() {
        let mut memory = Memory::new("TestAgent".to_string());
        memory.add_system_info("hostname: test");
        memory.add_interaction("query", "response");
        memory.add_tool_result("tool", "output");
        memory.add_observation("note");
        memory.add_error("warning");
        memory.add_topology("network");

        let ctx = memory.context();
        assert!(ctx.contains("TestAgent"));
        assert!(ctx.contains("system"));
        assert!(ctx.contains("tool"));
        assert!(ctx.contains("network"));
    }

    #[test]
    fn test_memory_store_and_recall() {
        let config = MemoryConfig {
            storage_dir: std::env::temp_dir(),
            ..Default::default()
        };
        let mut memory = Memory::new("test".to_string());
        memory.config = config;

        // Directly add to entries for testing
        memory.entries.push(MemoryEntry::new(
            "Deployed redis cluster".to_string(),
            vec![0.9, 0.1, 0.1],
        ));
        memory.entries.push(MemoryEntry::new(
            "Weather is nice".to_string(),
            vec![0.1, 0.9, 0.1],
        ));

        // Recall with query similar to entry1
        let results = memory.recall("redis deployment", &[0.85, 0.15, 0.1], 5);
        assert_eq!(results.len(), 2);
        // First result should be entry1 (more similar)
        assert!(results[0].content.contains("redis"));
    }
}

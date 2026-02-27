// Memory module - stores agent context and history

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Maximum number of journal entries to keep
const MAX_JOURNAL_ENTRIES: usize = 100;

/// Memory entry types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryEntry {
    /// System information (hostname, OS, etc.)
    SystemInfo(String),
    /// User interaction record
    UserInteraction { query: String, response: String },
    /// Tool execution result
    ToolResult { tool: String, result: String },
    /// Agent's own observation
    Observation(String),
    /// Error or warning
    Error(String),
}

/// Memory - stores agent's context
#[derive(Debug, Clone, Default)]
pub struct Memory {
    /// Journal entries (chronological record)
    journal: VecDeque<MemoryEntry>,
    /// Identity (static info about the agent)
    identity: String,
    /// Topology (known system structure)
    topology: Vec<String>,
}

impl Memory {
    /// Create new memory with identity
    pub fn new(identity: String) -> Self {
        Self {
            journal: VecDeque::new(),
            identity,
            topology: Vec::new(),
        }
    }

    /// Add entry to journal
    pub fn add(&mut self, entry: MemoryEntry) {
        self.journal.push_back(entry);
        // Trim if too large
        while self.journal.len() > MAX_JOURNAL_ENTRIES {
            self.journal.pop_front();
        }
    }

    /// Add system info
    pub fn add_system_info(&mut self, info: impl Into<String>) {
        self.add(MemoryEntry::SystemInfo(info.into()));
    }

    /// Add user interaction
    pub fn add_interaction(&mut self, query: impl Into<String>, response: impl Into<String>) {
        self.add(MemoryEntry::UserInteraction {
            query: query.into(),
            response: response.into(),
        });
    }

    /// Add tool result
    pub fn add_tool_result(&mut self, tool: impl Into<String>, result: impl Into<String>) {
        self.add(MemoryEntry::ToolResult {
            tool: tool.into(),
            result: result.into(),
        });
    }

    /// Add observation
    pub fn add_observation(&mut self, observation: impl Into<String>) {
        self.add(MemoryEntry::Observation(observation.into()));
    }

    /// Add error
    pub fn add_error(&mut self, error: impl Into<String>) {
        self.add(MemoryEntry::Error(error.into()));
    }

    /// Add topology info
    pub fn add_topology(&mut self, info: impl Into<String>) {
        self.topology.push(info.into());
    }

    /// Generate context string for system prompt
    pub fn context(&self) -> String {
        let mut parts = Vec::new();

        // Identity
        parts.push(format!("## Identity\n{}", self.identity));

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
    pub fn journal_entries(&self) -> Vec<&MemoryEntry> {
        self.journal.iter().collect()
    }
}

impl std::fmt::Display for MemoryEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryEntry::SystemInfo(s) => write!(f, "[system] {}", s),
            MemoryEntry::UserInteraction { query, response } => {
                write!(f, "[user] {} -> [response] {}", query, response)
            }
            MemoryEntry::ToolResult { tool, result } => {
                write!(f, "[tool: {}] {}", tool, result)
            }
            MemoryEntry::Observation(s) => write!(f, "[observation] {}", s),
            MemoryEntry::Error(s) => write!(f, "[error] {}", s),
        }
    }
}

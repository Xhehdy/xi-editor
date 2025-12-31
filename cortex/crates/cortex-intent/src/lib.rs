//! Cortex Intent - Capture, classify, and route user intentions
//!
//! Handles intent detection from typing patterns, commands, and comments.

mod parser;
mod ranking;

pub use parser::IntentParser;
pub use ranking::IntentRanker;

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use thiserror::Error;

/// Intent errors
#[derive(Debug, Error)]
pub enum IntentError {
    #[error("Failed to parse intent: {0}")]
    ParseError(String),
    #[error("Intent cancelled")]
    Cancelled,
    #[error("Intent queue full")]
    QueueFull,
}

/// Unique intent identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IntentId(pub u64);

static NEXT_INTENT_ID: AtomicU64 = AtomicU64::new(1);

impl IntentId {
    pub fn new() -> Self {
        Self(NEXT_INTENT_ID.fetch_add(1, Ordering::SeqCst))
    }
}

impl Default for IntentId {
    fn default() -> Self {
        Self::new()
    }
}

/// Symbol reference for intent context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolRef {
    pub name: String,
    pub file: Option<String>,
    pub line: Option<usize>,
}

/// Types of user intent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IntentType {
    /// Generate new code
    Generate { 
        prompt: String,
        context: Vec<SymbolRef>,
    },
    /// Refactor existing code
    Refactor {
        target: SymbolRef,
        instruction: String,
    },
    /// Explain code
    Explain {
        target: SymbolRef,
    },
    /// Debug an error
    Debug {
        error: String,
        context: Vec<SymbolRef>,
    },
    /// Plan a feature or change
    Plan {
        description: String,
    },
    /// Fix a TODO/FIXME
    Fix {
        description: String,
        location: SymbolRef,
    },
    /// Custom/unknown intent
    Custom {
        action: String,
        details: String,
    },
}

/// Source of the intent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IntentSource {
    /// Inferred from typing patterns
    Typing,
    /// Explicit command (e.g., cmd+shift+r)
    Command(String),
    /// Comment in code (e.g., // AI: make this async)
    Comment(String),
    /// Triggered by an agent
    Agent(String),
    /// From chat interface
    Chat,
}

/// A captured user intent
#[derive(Debug, Clone)]
pub struct Intent {
    pub id: IntentId,
    pub intent_type: IntentType,
    pub source: IntentSource,
    pub confidence: f32,
    pub cancelable: bool,
    pub created_at: Instant,
    pub file_context: Option<String>,
}

impl Intent {
    pub fn new(intent_type: IntentType, source: IntentSource) -> Self {
        Self {
            id: IntentId::new(),
            intent_type,
            source,
            confidence: 1.0,
            cancelable: true,
            created_at: Instant::now(),
            file_context: None,
        }
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    pub fn with_context(mut self, file: String) -> Self {
        self.file_context = Some(file);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intent_creation() {
        let intent = Intent::new(
            IntentType::Generate {
                prompt: "Create a login function".to_string(),
                context: vec![],
            },
            IntentSource::Chat,
        );
        
        assert!(intent.confidence == 1.0);
        assert!(intent.cancelable);
    }
}

//! Intent parser - Extracts intents from code comments and text

use crate::{Intent, IntentSource, IntentType, SymbolRef};
use regex::Regex;
use std::sync::OnceLock;

/// Patterns for extracting intents from comments
static TODO_PATTERN: OnceLock<Regex> = OnceLock::new();
static FIXME_PATTERN: OnceLock<Regex> = OnceLock::new();
static AI_PATTERN: OnceLock<Regex> = OnceLock::new();

fn todo_pattern() -> &'static Regex {
    TODO_PATTERN.get_or_init(|| Regex::new(r"(?i)//\s*TODO:\s*(.+)$").unwrap())
}

fn fixme_pattern() -> &'static Regex {
    FIXME_PATTERN.get_or_init(|| Regex::new(r"(?i)//\s*FIXME:\s*(.+)$").unwrap())
}

fn ai_pattern() -> &'static Regex {
    AI_PATTERN.get_or_init(|| Regex::new(r"(?i)//\s*AI:\s*(.+)$").unwrap())
}

/// Parser for extracting intents from code
pub struct IntentParser {
    file_path: Option<String>,
}

impl IntentParser {
    pub fn new() -> Self {
        Self { file_path: None }
    }

    pub fn with_file(mut self, path: String) -> Self {
        self.file_path = Some(path);
        self
    }

    /// Parse all intents from a block of code
    pub fn parse(&self, code: &str) -> Vec<Intent> {
        let mut intents = Vec::new();

        for (line_num, line) in code.lines().enumerate() {
            if let Some(intent) = self.parse_line(line, line_num + 1) {
                intents.push(intent);
            }
        }

        intents
    }

    /// Parse a single line for intent
    pub fn parse_line(&self, line: &str, line_number: usize) -> Option<Intent> {
        // Check for AI: comments (highest priority)
        if let Some(caps) = ai_pattern().captures(line) {
            let instruction = caps.get(1)?.as_str().trim().to_string();
            return Some(self.create_intent_from_ai_comment(&instruction, line_number));
        }

        // Check for TODO: comments
        if let Some(caps) = todo_pattern().captures(line) {
            let description = caps.get(1)?.as_str().trim().to_string();
            return Some(self.create_fix_intent(&description, line_number, "TODO"));
        }

        // Check for FIXME: comments
        if let Some(caps) = fixme_pattern().captures(line) {
            let description = caps.get(1)?.as_str().trim().to_string();
            return Some(self.create_fix_intent(&description, line_number, "FIXME"));
        }

        None
    }

    fn create_intent_from_ai_comment(&self, instruction: &str, line_number: usize) -> Intent {
        let lower = instruction.to_lowercase();

        let intent_type = if lower.starts_with("refactor") || lower.starts_with("change") {
            IntentType::Refactor {
                target: self.symbol_ref_at_line(line_number),
                instruction: instruction.to_string(),
            }
        } else if lower.starts_with("explain") {
            IntentType::Explain {
                target: self.symbol_ref_at_line(line_number),
            }
        } else if lower.starts_with("debug") || lower.starts_with("fix") {
            IntentType::Debug {
                error: instruction.to_string(),
                context: vec![self.symbol_ref_at_line(line_number)],
            }
        } else if lower.starts_with("plan") {
            IntentType::Plan {
                description: instruction.to_string(),
            }
        } else {
            // Default to generate
            IntentType::Generate {
                prompt: instruction.to_string(),
                context: vec![self.symbol_ref_at_line(line_number)],
            }
        };

        let mut intent = Intent::new(intent_type, IntentSource::Comment(instruction.to_string()));
        if let Some(ref path) = self.file_path {
            intent = intent.with_context(path.clone());
        }
        intent
    }

    fn create_fix_intent(&self, description: &str, line_number: usize, tag: &str) -> Intent {
        let intent_type = IntentType::Fix {
            description: description.to_string(),
            location: self.symbol_ref_at_line(line_number),
        };

        let mut intent = Intent::new(
            intent_type,
            IntentSource::Comment(format!("{}: {}", tag, description)),
        );
        intent = intent.with_confidence(0.7); // Lower confidence for TODO/FIXME

        if let Some(ref path) = self.file_path {
            intent = intent.with_context(path.clone());
        }
        intent
    }

    fn symbol_ref_at_line(&self, line_number: usize) -> SymbolRef {
        SymbolRef {
            name: format!("line_{}", line_number),
            file: self.file_path.clone(),
            line: Some(line_number),
        }
    }
}

impl Default for IntentParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ai_comment() {
        let parser = IntentParser::new().with_file("test.rs".to_string());
        let intents = parser.parse("// AI: make this function async");

        assert_eq!(intents.len(), 1);
        assert!(matches!(
            intents[0].intent_type,
            IntentType::Generate { .. }
        ));
    }

    #[test]
    fn test_parse_todo() {
        let parser = IntentParser::new();
        let intents = parser.parse("// TODO: implement error handling");

        assert_eq!(intents.len(), 1);
        assert!(matches!(intents[0].intent_type, IntentType::Fix { .. }));
    }

    #[test]
    fn test_parse_refactor() {
        let parser = IntentParser::new();
        let intents = parser.parse("// AI: refactor to use async/await");

        assert_eq!(intents.len(), 1);
        assert!(matches!(
            intents[0].intent_type,
            IntentType::Refactor { .. }
        ));
    }

    #[test]
    fn test_parse_multiple() {
        let parser = IntentParser::new();
        let code = r#"
// TODO: add logging
fn hello() {}
// AI: explain this function
// FIXME: handle errors
"#;
        let intents = parser.parse(code);
        assert_eq!(intents.len(), 3);
    }
}

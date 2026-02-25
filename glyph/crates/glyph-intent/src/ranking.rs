//! Intent ranking - Prioritizes and scores intents

use crate::{Intent, IntentSource, IntentType};
use std::cmp::Ordering;

/// Ranks and prioritizes intents
pub struct IntentRanker;

impl IntentRanker {
    /// Compare two intents for priority ordering
    /// Returns Ordering::Greater if `a` should be processed before `b`
    pub fn compare(a: &Intent, b: &Intent) -> Ordering {
        // First compare by confidence
        let conf_cmp = b
            .confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(Ordering::Equal);

        if conf_cmp != Ordering::Equal {
            return conf_cmp;
        }

        // Then by source priority
        let source_a = Self::source_priority(&a.source);
        let source_b = Self::source_priority(&b.source);

        if source_a != source_b {
            return source_b.cmp(&source_a);
        }

        // Then by type priority
        let type_a = Self::type_priority(&a.intent_type);
        let type_b = Self::type_priority(&b.intent_type);

        if type_a != type_b {
            return type_b.cmp(&type_a);
        }

        // Finally by creation time (older first)
        a.created_at.cmp(&b.created_at)
    }

    /// Get priority score for intent source (higher = more urgent)
    fn source_priority(source: &IntentSource) -> u8 {
        match source {
            IntentSource::Command(_) => 100, // Explicit commands are highest
            IntentSource::Chat => 90,        // Chat requests are high priority
            IntentSource::Agent(_) => 80,    // Agent-initiated
            IntentSource::Comment(_) => 50,  // Comments are medium
            IntentSource::Typing => 30,      // Typing inference is low
        }
    }

    /// Get priority score for intent type (higher = more urgent)
    fn type_priority(intent_type: &IntentType) -> u8 {
        match intent_type {
            IntentType::Debug { .. } => 100,   // Debugging is urgent
            IntentType::Fix { .. } => 90,      // Fixes are high priority
            IntentType::Refactor { .. } => 70, // Refactors are medium-high
            IntentType::Generate { .. } => 60, // Generation is medium
            IntentType::Explain { .. } => 50,  // Explanation is medium
            IntentType::Plan { .. } => 40,     // Planning is lower
            IntentType::Custom { .. } => 30,   // Custom is lowest
        }
    }

    /// Sort a list of intents by priority (highest first)
    pub fn sort(intents: &mut [Intent]) {
        intents.sort_by(Self::compare);
    }

    /// Filter intents by minimum confidence
    pub fn filter_by_confidence(intents: Vec<Intent>, min_confidence: f32) -> Vec<Intent> {
        intents
            .into_iter()
            .filter(|i| i.confidence >= min_confidence)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_intent(intent_type: IntentType, source: IntentSource, confidence: f32) -> Intent {
        Intent::new(intent_type, source).with_confidence(confidence)
    }

    #[test]
    fn test_confidence_ordering() {
        let high = make_intent(
            IntentType::Generate {
                prompt: "a".into(),
                context: vec![],
            },
            IntentSource::Chat,
            0.9,
        );
        let low = make_intent(
            IntentType::Generate {
                prompt: "b".into(),
                context: vec![],
            },
            IntentSource::Chat,
            0.5,
        );

        assert_eq!(IntentRanker::compare(&high, &low), Ordering::Less); // high comes first
    }

    #[test]
    fn test_source_ordering() {
        let command = make_intent(
            IntentType::Generate {
                prompt: "a".into(),
                context: vec![],
            },
            IntentSource::Command("test".into()),
            1.0,
        );
        let chat = make_intent(
            IntentType::Generate {
                prompt: "b".into(),
                context: vec![],
            },
            IntentSource::Chat,
            1.0,
        );

        assert_eq!(IntentRanker::compare(&command, &chat), Ordering::Less); // command comes first
    }

    #[test]
    fn test_sort() {
        let mut intents = vec![
            make_intent(
                IntentType::Plan {
                    description: "low".into(),
                },
                IntentSource::Typing,
                0.5,
            ),
            make_intent(
                IntentType::Debug {
                    error: "high".into(),
                    context: vec![],
                },
                IntentSource::Command("fix".into()),
                1.0,
            ),
        ];

        IntentRanker::sort(&mut intents);

        // Debug with Command should be first
        assert!(matches!(intents[0].intent_type, IntentType::Debug { .. }));
    }
}

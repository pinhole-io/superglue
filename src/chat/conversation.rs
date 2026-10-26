//! Stateful multi-turn chat: owns caller-visible [`ChatMessage`] history.

use crate::guardrails::GuardrailRegistry;
use crate::hooks::HookRegistry;
use crate::http::HttpClient;
use crate::openai::ChatMessage;
use crate::tools::ToolRegistry;

use super::{ChatError, ChatOptions, CompletionOutcome, complete_with_tools};

/// OpenAI-style message list managed across [`Self::complete`] turns.
///
/// Does not include the synthetic `system` row from [`ChatOptions::system_prompt`];
/// that row is still prepended on each HTTP request inside [`complete_with_tools`].
#[derive(Debug, Clone, Default)]
pub struct Conversation {
    pub messages: Vec<ChatMessage>,
}

impl Conversation {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_messages(messages: Vec<ChatMessage>) -> Self {
        Self { messages }
    }

    pub fn push_user(&mut self, text: impl Into<String>) {
        self.messages.push(ChatMessage::text("user", text.into()));
    }

    /// Push a user message that may contain file and image parts.
    pub fn push_user_message(&mut self, mut message: ChatMessage) {
        message.role = "user".to_string();
        self.messages.push(message);
    }

    pub fn push_assistant_text(&mut self, text: impl Into<String>) {
        self.messages
            .push(ChatMessage::text("assistant", text.into()));
    }

    /// Runs the tool loop for the current [`Self::messages`] buffer and replaces it
    /// with the post-turn caller-visible history from [`CompletionOutcome::messages`].
    pub async fn complete(
        &mut self,
        http: &HttpClient,
        registry: &ToolRegistry,
        hooks: &HookRegistry,
        guardrails: &GuardrailRegistry,
        options: &ChatOptions,
    ) -> Result<CompletionOutcome, ChatError> {
        let outcome = complete_with_tools(
            http,
            registry,
            hooks,
            guardrails,
            self.messages.clone(),
            options,
        )
        .await?;
        self.messages = outcome.messages.clone();
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::{ContentPart, MessageContent};

    #[test]
    fn push_user_message_keeps_the_parts() {
        let mut conversation = Conversation::new();
        let mut message = ChatMessage::default();
        message.role = "assistant".to_string();
        message.content = Some(MessageContent::Parts(vec![ContentPart::Text {
            text: "See note.pdf.".to_string(),
        }]));
        conversation.push_user_message(message);
        assert_eq!(conversation.messages.len(), 1);
        assert_eq!(conversation.messages[0].role, "user");
        match &conversation.messages[0].content {
            Some(MessageContent::Parts(parts)) => {
                assert!(
                    matches!(parts.first(), Some(ContentPart::Text { text }) if text == "See note.pdf.")
                );
            }
            _ => panic!("the parts stay on the message"),
        }
    }
}

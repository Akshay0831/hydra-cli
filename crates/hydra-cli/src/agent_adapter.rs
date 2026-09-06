//! Hydra-owned boundary around the upstream Pi embedding API.

use anyhow::Result;
use pi::model::AssistantMessageEvent;
use pi::sdk::{AgentEvent, SessionOptions};
use std::path::PathBuf;

/// Inputs Hydra needs to start one Pi agent prompt.
pub struct PromptRequest {
    pub message: String,
    pub tools: Vec<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub working_directory: PathBuf,
}

/// Minimal Hydra-owned facade for the upstream agent SDK.
pub struct AgentAdapter;

impl AgentAdapter {
    pub fn builtin_tool_names() -> &'static [&'static str] {
        pi::sdk::BUILTIN_TOOL_NAMES
    }

    pub async fn prompt(
        request: PromptRequest,
        on_text: impl Fn(&str) + Send + Sync + 'static,
    ) -> Result<()> {
        let options = SessionOptions {
            provider: request.provider,
            model: request.model,
            api_key: request.api_key,
            enabled_tools: Some(request.tools),
            working_directory: Some(request.working_directory),
            ..SessionOptions::default()
        };
        let mut session = pi::sdk::create_agent_session(options).await?;
        session
            .prompt(request.message, move |event| {
                if let AgentEvent::MessageUpdate {
                    assistant_message_event: AssistantMessageEvent::TextDelta { delta, .. },
                    ..
                } = event
                {
                    on_text(&delta);
                }
            })
            .await?;
        Ok(())
    }
}

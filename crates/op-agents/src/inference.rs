//! Text-only provider completion: no tool, function, plugin, delegate or model selector input.
use anyhow::{Result,ensure,Context};
use schemars::JsonSchema;
use serde::{Serialize,Deserialize};
use crate::judgment::Usage;
#[derive(Debug,Clone,Serialize,Deserialize,JsonSchema)]
#[serde(rename_all="snake_case")]
pub enum TextRole { System, User, Assistant }
#[derive(Debug,Clone,Serialize,Deserialize,JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextMessage {pub role:TextRole,pub content:String}
#[derive(Debug,Clone,Serialize,Deserialize,JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompleteTextInput {
    #[schemars(length(min=1,max=128))]
    pub messages:Vec<TextMessage>,
    #[schemars(range(min=1,max=32768))]
    pub max_output_tokens:u32,
}
impl CompleteTextInput {
    pub fn validate(&self)->Result<()> {
        ensure!(!self.messages.is_empty()&&self.messages.len()<=128&&(1..=32768).contains(&self.max_output_tokens),"text completion bounds invalid");
        let bytes=self.messages.iter().try_fold(0usize,|sum,message|sum.checked_add(message.content.len()).context("text context overflow"))?;
        ensure!(bytes<=128*1024&&self.messages.iter().any(|m|!m.content.trim().is_empty()),"text context empty or exceeds bound");Ok(())
    }
}
#[derive(Debug,Clone,Serialize,Deserialize,JsonSchema)]
#[serde(rename_all="snake_case")]
pub enum CompletionStatus { Complete, Truncated }
#[derive(Debug,Clone,Serialize,Deserialize,JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompleteTextOutput {
    pub content:String,pub model:String,pub provider_reference:String,pub status:CompletionStatus,pub usage:Usage,
}
#[derive(Debug,Clone,Serialize,Deserialize,JsonSchema,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
pub struct NativeProviderProfile {
    pub provider_reference:String,pub model:String,pub http_uri:String,pub wire_api:Option<String>,
}

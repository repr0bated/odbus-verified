//! ⚖️ Tool Profiles — R14
//!
//! Controls which tools are exposed to agents to reduce token cost.
//! Minimal (5), Standard (10), Full (16).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolProfile {
    Minimal,
    #[default]
    Standard,
    Full,
}

impl std::fmt::Display for ToolProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Minimal => write!(f, "minimal"),
            Self::Standard => write!(f, "standard"),
            Self::Full => write!(f, "full"),
        }
    }
}

impl std::str::FromStr for ToolProfile {
    type Err = ();
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "minimal" | "min" => Self::Minimal,
            "standard" | "std" => Self::Standard,
            "full" | "all" => Self::Full,
            _ => Self::Standard,
        })
    }
}

pub fn tools_for_profile(profile: ToolProfile) -> Vec<&'static str> {
    match profile {
        ToolProfile::Minimal => vec![
            "ask_question",
            "list_notebooks",
            "select_notebook",
            "get_notebook",
            "get_health",
        ],
        ToolProfile::Standard => vec![
            "ask_question",
            "query_notebook",
            "list_notebooks",
            "select_notebook",
            "get_notebook",
            "add_source_url",
            "add_source_text",
            "list_sources",
            "get_source_content",
            "get_health",
        ],
        ToolProfile::Full => vec![
            "ask_question",
            "query_notebook",
            "list_notebooks",
            "select_notebook",
            "get_notebook",
            "create_notebook",
            "batch_create_notebooks",
            "add_source_url",
            "add_source_text",
            "add_folder",
            "list_sources",
            "remove_source",
            "get_source_content",
            "generate_data_table",
            "get_health",
            "doctor",
        ],
    }
}

pub fn is_tool_allowed(profile: ToolProfile, tool_name: &str) -> bool {
    tools_for_profile(profile).contains(&tool_name)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileTokenEstimate {
    pub tool_count: u32,
    pub schema_tokens: u32,
    pub savings_percent: u32,
}

pub fn token_estimate(profile: ToolProfile) -> ProfileTokenEstimate {
    match profile {
        ToolProfile::Minimal => ProfileTokenEstimate {
            tool_count: 5,
            schema_tokens: 800,
            savings_percent: 69,
        },
        ToolProfile::Standard => ProfileTokenEstimate {
            tool_count: 10,
            schema_tokens: 1600,
            savings_percent: 38,
        },
        ToolProfile::Full => ProfileTokenEstimate {
            tool_count: 16,
            schema_tokens: 2600,
            savings_percent: 0,
        },
    }
}

pub fn current_profile() -> ToolProfile {
    std::env::var("COGNITIVE_MCP_TOOL_PROFILE")
        .or_else(|_| std::env::var("NOTEBOOKLM_PROFILE"))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_return_correct_tool_counts() {
        assert_eq!(tools_for_profile(ToolProfile::Minimal).len(), 5);
        assert_eq!(tools_for_profile(ToolProfile::Standard).len(), 10);
        assert_eq!(tools_for_profile(ToolProfile::Full).len(), 16);
    }

    #[test]
    fn should_check_tool_allowed() {
        assert!(is_tool_allowed(ToolProfile::Minimal, "ask_question"));
        assert!(!is_tool_allowed(ToolProfile::Minimal, "doctor"));
        assert!(is_tool_allowed(ToolProfile::Full, "doctor"));
    }

    #[test]
    fn should_parse_profile_names() {
        assert_eq!(
            "minimal".parse::<ToolProfile>().unwrap(),
            ToolProfile::Minimal
        );
        assert_eq!("full".parse::<ToolProfile>().unwrap(), ToolProfile::Full);
    }

    #[test]
    fn should_estimate_token_savings() {
        let minimal = token_estimate(ToolProfile::Minimal);
        let full = token_estimate(ToolProfile::Full);
        assert!(minimal.savings_percent > 0);
        assert_eq!(full.savings_percent, 0);
    }
}

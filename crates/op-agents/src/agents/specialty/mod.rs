//! Specialty Agents
//!
//! Niche and specialized domain agents

mod arm_cortex_expert;
mod error_detective;
mod hybrid_cloud_architect;
mod legacy_modernizer;
mod observability_engineer;
mod quant_analyst;
mod snowball_developer;
mod ui_ux_designer;
mod unity_developer;

pub use arm_cortex_expert::ARMCortexExpertAgent;
pub use error_detective::ErrorDetectiveAgent;
pub use hybrid_cloud_architect::HybridCloudArchitectAgent;
pub use legacy_modernizer::LegacyModernizerAgent;
pub use observability_engineer::ObservabilityEngineerAgent;
pub use quant_analyst::QuantAnalystAgent;
pub use snowball_developer::SnowballDeveloperAgent;
pub use ui_ux_designer::UIUXDesignerAgent;
pub use unity_developer::UnityDeveloperAgent;

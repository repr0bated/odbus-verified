//! AI/ML Agents
//!
//! Specialized agents for AI and machine learning:
//! - `AIEngineer`: LLM applications, RAG systems, prompt engineering
//! - `MLEngineer`: Model training, optimization, deployment
//! - `MLOpsEngineer`: ML pipelines, model serving, monitoring
//! - `DataScientist`: Data analysis, visualization, experimentation
//! - `PromptEngineer`: Prompt design, optimization, evaluation

mod ai_engineer;
mod data_engineer;
mod data_scientist;
mod ml_engineer;
mod mlops_engineer;
pub mod prompt_engineer;

pub use ai_engineer::AIEngineerAgent;
pub use data_engineer::DataEngineerAgent;
pub use data_scientist::DataScientistAgent;
pub use ml_engineer::MLEngineerAgent;
pub use mlops_engineer::MLOpsEngineerAgent;
pub use prompt_engineer::PromptEngineerAgent;

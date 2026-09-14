//! Web Framework Agents
//!
//! Specialized agents for web framework development:
//! - `DjangoPro`: Django web framework expert
//! - `FastAPIPro`: FastAPI framework expert
//! - `TemporalPythonPro`: Temporal workflow expert

mod django_pro;
mod fastapi_pro;
mod temporal_python_pro;

pub use django_pro::DjangoProAgent;
pub use fastapi_pro::FastAPIProAgent;
pub use temporal_python_pro::TemporalPythonProAgent;

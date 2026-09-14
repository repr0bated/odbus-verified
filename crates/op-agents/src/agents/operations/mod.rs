//! Operations Agents
//!
//! Specialized agents for operations and SRE:
//! - `IncidentResponder`: Production incident management and resolution
//! - `DevOpsTroubleshooter`: System debugging and troubleshooting
//! - `TestAutomator`: Test suite creation and automation
//! - `ObservabilityEngineer`: Monitoring, logging, and tracing

mod devops_troubleshooter;
mod incident_responder;
mod test_automator;

pub use devops_troubleshooter::DevOpsTroubleshooterAgent;
pub use incident_responder::IncidentResponderAgent;
pub use test_automator::TestAutomatorAgent;

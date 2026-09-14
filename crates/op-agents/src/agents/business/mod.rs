//! Business & Operations Agents
//!
//! - `BusinessAnalyst`: Business analysis and requirements
//! - `HRPro`: HR policies and compliance
//! - `CustomerSupport`: Customer service automation
//! - `PaymentIntegration`: Payment processing integration

mod business_analyst;
mod customer_support;
mod hr_pro;
mod legal_advisor;
mod payment_integration;
mod sales_automator;

pub use business_analyst::BusinessAnalystAgent;
pub use customer_support::CustomerSupportAgent;
pub use hr_pro::HRProAgent;
pub use legal_advisor::LegalAdvisorAgent;
pub use payment_integration::PaymentIntegrationAgent;
pub use sales_automator::SalesAutomatorAgent;

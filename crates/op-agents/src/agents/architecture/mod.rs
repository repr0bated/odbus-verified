//! Architecture Agents
//!
//! Specialized agents for software architecture design:
//! - `BackendArchitect`: API design, microservices, distributed systems
//! - `GraphQLArchitect`: GraphQL schema design, federation, performance
//! - `FrontendDeveloper`: React, Next.js, modern frontend patterns

mod backend_architect;
mod frontend_developer;
mod graphql_architect;

pub use backend_architect::BackendArchitectAgent;
pub use frontend_developer::FrontendDeveloperAgent;
pub use graphql_architect::GraphQLArchitectAgent;

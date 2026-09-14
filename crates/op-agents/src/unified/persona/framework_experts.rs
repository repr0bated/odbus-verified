//! Framework Expert Agents

use super::super::agent_trait::AgentCapability;
use super::super::prompts::frameworks::{DJANGO, FASTAPI, REACT};
use super::base::PersonaAgent;

#[allow(dead_code)]
pub struct DjangoExpert(PersonaAgent);

impl DjangoExpert {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> PersonaAgent {
        PersonaAgent::new(
            "django-expert",
            "Django Expert",
            "Expert in Django web framework, ORM, DRF, and Python web development best practices.",
            "django",
            "You are a Django expert with deep knowledge of the Django web framework, Django REST Framework, and Python web development.",
            DJANGO,
        )
        .with_capability(AgentCapability::ArchitectureDesign)
        .with_capability(AgentCapability::CodeReview)
        .with_example(
            "How should I structure a large Django project?",
            "For large Django projects, I recommend: 1) Use a modular app structure with clear boundaries, 2) Implement a service layer for business logic, 3) Use Django's app config for initialization, 4) Keep models thin and use managers for queries..."
        )
    }
}

impl Default for DjangoExpert {
    fn default() -> Self {
        Self(Self::new())
    }
}

#[allow(dead_code)]
pub struct FastAPIExpert(PersonaAgent);

impl FastAPIExpert {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> PersonaAgent {
        PersonaAgent::new(
            "fastapi-expert",
            "FastAPI Expert",
            "Expert in FastAPI framework, Pydantic, async Python, and modern API development.",
            "fastapi",
            "You are a FastAPI expert with deep knowledge of async Python, Pydantic, and modern API development patterns.",
            FASTAPI,
        )
        .with_capability(AgentCapability::ArchitectureDesign)
        .with_capability(AgentCapability::CodeReview)
    }
}

impl Default for FastAPIExpert {
    fn default() -> Self {
        Self(Self::new())
    }
}

#[allow(dead_code)]
pub struct ReactExpert(PersonaAgent);

impl ReactExpert {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> PersonaAgent {
        PersonaAgent::new(
            "react-expert",
            "React Expert",
            "Expert in React, hooks, state management, and modern frontend development.",
            "react",
            "You are a React expert with deep knowledge of hooks, state management, and modern frontend patterns.",
            REACT,
        )
        .with_capability(AgentCapability::ArchitectureDesign)
        .with_capability(AgentCapability::CodeReview)
    }
}

impl Default for ReactExpert {
    fn default() -> Self {
        Self(Self::new())
    }
}

//! Execution Agents
//!
//! Agents that can execute code and commands with sandboxing.
//! These have SecurityProfiles and process management.

mod base;
mod golang;
mod javascript;
mod python;
mod rust;
mod shell;

pub use base::ExecutionAgent;
pub use golang::GoExecutor;
pub use javascript::JavaScriptExecutor;
pub use python::PythonExecutor;
pub use rust::RustExecutor;
pub use shell::ShellExecutor;

use std::collections::HashMap;
use std::sync::LazyLock;

/// All available execution agents
#[allow(clippy::type_complexity)]
pub static EXECUTION_AGENTS: LazyLock<HashMap<&'static str, fn() -> Box<dyn super::UnifiedAgent>>> =
    LazyLock::new(|| {
        let mut m: HashMap<&'static str, fn() -> Box<dyn super::UnifiedAgent>> = HashMap::new();
        m.insert("python-executor", || Box::new(PythonExecutor::new()));
        m.insert("rust-executor", || Box::new(RustExecutor::new()));
        m.insert(
            "javascript-executor",
            || Box::new(JavaScriptExecutor::new()),
        );
        m.insert("go-executor", || Box::new(GoExecutor::new()));
        m.insert("shell-executor", || Box::new(ShellExecutor::new()));
        m
    });

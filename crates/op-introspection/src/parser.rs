//! Introspection XML parser

use op_core::{ObjectInfo, Result};

pub struct IntrospectionParser;

impl IntrospectionParser {
    pub fn new() -> Self {
        Self
    }

    pub fn parse(&self, _xml: &str, path: &str) -> Result<ObjectInfo> {
        // Parsing is done in scanner module
        Ok(ObjectInfo {
            path: path.to_string(),
            interfaces: Vec::new(),
            children: Vec::new(),
        })
    }
}

impl Default for IntrospectionParser {
    fn default() -> Self {
        Self::new()
    }
}

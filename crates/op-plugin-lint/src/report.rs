use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Fail,
    Warn,
    Hint,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub code: String,
    pub message: String,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub source: String,
    pub findings: Vec<Finding>,
}

impl Report {
    pub fn ok(&self) -> bool {
        !self.findings.iter().any(|f| f.severity == Severity::Fail)
    }

    pub fn fail(
        &mut self,
        code: impl Into<String>,
        message: impl Into<String>,
        location: impl Into<Option<String>>,
    ) {
        self.findings.push(Finding {
            severity: Severity::Fail,
            code: code.into(),
            message: message.into(),
            location: location.into(),
        });
    }

    pub fn warn(
        &mut self,
        code: impl Into<String>,
        message: impl Into<String>,
        location: impl Into<Option<String>>,
    ) {
        self.findings.push(Finding {
            severity: Severity::Warn,
            code: code.into(),
            message: message.into(),
            location: location.into(),
        });
    }

    pub fn hint(
        &mut self,
        code: impl Into<String>,
        message: impl Into<String>,
        location: impl Into<Option<String>>,
    ) {
        self.findings.push(Finding {
            severity: Severity::Hint,
            code: code.into(),
            message: message.into(),
            location: location.into(),
        });
    }

    pub fn to_markdown(&self) -> String {
        let status = if self.ok() { "PASS" } else { "FAIL" };
        let mut out = String::new();
        out.push_str(&format!(
            "# Plugin render-contract audit: {}\n\n",
            self.source
        ));
        out.push_str(&format!("**Status:** {status}\n\n"));
        if self.findings.is_empty() {
            out.push_str("No findings.\n");
            return out;
        }
        out.push_str("| Severity | Code | Location | Message |\n");
        out.push_str("|---|---|---|---|\n");
        for f in &self.findings {
            let sev = match f.severity {
                Severity::Fail => "FAIL",
                Severity::Warn => "WARN",
                Severity::Hint => "HINT",
            };
            let loc = f.location.as_deref().unwrap_or("—");
            let msg = f.message.replace('|', "\\|").replace('\n', " ");
            out.push_str(&format!("| {sev} | `{}` | `{loc}` | {msg} |\n", f.code));
        }
        out.push('\n');
        out
    }

    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

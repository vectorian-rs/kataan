#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticReport {
    pub diagnostics: Vec<Diagnostic>,
}

impl DiagnosticReport {
    pub fn new(diagnostics: Vec<Diagnostic>) -> Self {
        Self { diagnostics }
    }

    pub fn is_ok(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub message: String,
    pub path: Option<String>,
}

impl Diagnostic {
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, code, message)
    }

    pub fn warning(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, code, message)
    }

    pub fn info(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(Severity::Info, code, message)
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    fn new(severity: Severity, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity,
            code: code.into(),
            message: message.into(),
            path: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_is_valid_when_no_error_diagnostics_exist() {
        let report = DiagnosticReport::new(vec![
            Diagnostic::warning("index-drift", "Index is stale").with_path("projects/index.toml"),
            Diagnostic::info("checksum-refreshed", "Checksum refreshed"),
        ]);

        assert!(report.is_ok());
    }

    #[test]
    fn report_is_invalid_when_error_diagnostic_exists() {
        let report = DiagnosticReport::new(vec![Diagnostic::error(
            "missing-toml-sidecar",
            "Markdown file is missing a TOML sidecar",
        )]);

        assert!(!report.is_ok());
    }
}

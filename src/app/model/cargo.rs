use serde::{self, Deserialize, Serialize};

// Cargo JSON message format models
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "reason", rename_all = "kebab-case")]
pub enum CargoMessage {
    CompilerMessage {
        message: DiagnosticMessage,
        #[serde(default)]
        target: Option<Target>,
    },
    CompilerArtifact {
        #[serde(default)]
        target: Option<Target>,
        #[serde(default)]
        filenames: Vec<String>,
        #[serde(default)]
        fresh: bool,
    },
    BuildScriptExecuted {
        #[serde(default)]
        package_id: String,
    },
    BuildFinished {
        success: bool,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiagnosticMessage {
    /// The primary message (e.g., "unused variable: `x`")
    pub message: String,
    /// The level: "error", "warning", "note", "help", etc.
    pub level: String,
    /// ANSI-formatted rendered output
    #[serde(default)]
    pub rendered: Option<String>,
    /// Spans showing where in the code the issue is
    #[serde(default)]
    pub spans: Vec<DiagnosticSpan>,
    /// Child diagnostics (notes, help messages, etc.)
    #[serde(default)]
    pub children: Vec<DiagnosticMessage>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiagnosticSpan {
    /// File path where the diagnostic points
    #[serde(default)]
    pub file_name: String,
    /// Line number (1-indexed)
    #[serde(default)]
    pub line_start: usize,
    /// Column number (1-indexed)
    #[serde(default)]
    pub column_start: usize,
    /// Whether this is the primary span
    #[serde(default)]
    pub is_primary: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Target {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub kind: Vec<String>,
}

use serde::{self, Deserialize, Serialize};

use super::DiagnosticDisplayMode;

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

impl CargoMessage {
    /// Parse a line of output as a CargoMessage
    pub fn parse(line: &str) -> Option<Self> {
        if line.trim().starts_with('{') {
            serde_json::from_str::<CargoMessage>(line).ok()
        } else {
            None
        }
    }

    /// Check if this message is a compiler diagnostic
    pub fn is_compiler_diagnostic(&self) -> bool {
        matches!(self, CargoMessage::CompilerMessage { .. })
    }

    /// Render the cargo message to a vector of output lines
    pub fn render(&self, mode: &DiagnosticDisplayMode, is_first: bool) -> Vec<String> {
        match self {
            CargoMessage::CompilerMessage { message, target } => {
                let mut output = Vec::new();

                // Use the rendered field if available, otherwise format manually
                if let Some(rendered) = &message.rendered {
                    // The rendered field contains the full ANSI-formatted diagnostic
                    match mode {
                        DiagnosticDisplayMode::Summary => {
                            // Show only first line and file location
                            if let Some(first_line) = rendered.lines().next() {
                                output.push(first_line.to_string());
                            }
                            // Add file location from primary span
                            if let Some(span) = message.spans.iter().find(|s| s.is_primary) {
                                output.push(format!(
                                    "  --> {}:{}:{}",
                                    span.file_name, span.line_start, span.column_start
                                ));
                            }
                        }
                        DiagnosticDisplayMode::First => {
                            // Show full for first diagnostic, summary for rest
                            if is_first {
                                for line in rendered.lines() {
                                    output.push(line.to_string());
                                }
                            } else if let Some(first_line) = rendered.lines().next() {
                                let span = message
                                    .spans
                                    .first()
                                    .map(|span| {
                                        format!(
                                            "{}:{}:{}",
                                            span.file_name, span.line_start, span.column_start
                                        )
                                    })
                                    .unwrap_or("".to_string());
                                output.push(format!("{first_line} {span}"));
                            }
                        }
                        DiagnosticDisplayMode::Full => {
                            // Show all lines for all diagnostics
                            for line in rendered.lines() {
                                output.push(line.to_string());
                            }
                        }
                    }
                } else {
                    // Fallback: manually format the message
                    let level_prefix = match message.level {
                        DiagnosticLevel::Error => "error",
                        DiagnosticLevel::Warning => "warning",
                        DiagnosticLevel::Note => "note",
                        DiagnosticLevel::Help => "help",
                        _ => "",
                    };

                    if let Some(target) = target {
                        output.push(format!(
                            "[{}] {}: {}",
                            target.name, level_prefix, message.message
                        ));
                    } else {
                        output.push(format!("{}: {}", level_prefix, message.message));
                    }

                    // Add file location in Summary mode
                    if matches!(mode, DiagnosticDisplayMode::Summary)
                        && let Some(span) = message.spans.iter().find(|s| s.is_primary)
                    {
                        output.push(format!(
                            "  --> {}:{}:{}",
                            span.file_name, span.line_start, span.column_start
                        ));
                    }
                }

                output
            }
            CargoMessage::CompilerArtifact { .. } => {
                vec![] // Skip artifact messages
            }
            CargoMessage::BuildScriptExecuted { .. } => {
                vec![] // Skip build script messages
            }
            CargoMessage::BuildFinished { success } => {
                if *success {
                    vec!["   Build finished successfully".to_string()]
                } else {
                    vec!["   Build failed".to_string()]
                }
            }
            CargoMessage::Unknown => {
                vec![] // Skip unknown messages
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiagnosticMessage {
    /// The primary message (e.g., "unused variable: `x`")
    pub message: String,
    /// The level: "error", "warning", "note", "help", etc.
    pub level: DiagnosticLevel,
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticLevel {
    Error,
    Warning,
    Note,
    Help,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Target {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub kind: Vec<String>,
}

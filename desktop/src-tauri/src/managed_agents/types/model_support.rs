use serde::Serialize;

/// Function-tool compatibility reported for a discovered model.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentModelToolSupport {
    /// Function-tool support is explicit.
    Supported,
    /// Function tools are explicitly unavailable.
    Unsupported,
    /// No capability signal was available.
    #[default]
    Unknown,
}

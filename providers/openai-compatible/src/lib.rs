//! Library target for the OpenAI-compatible provider, re-exporting the
//! modules so integration tests under `tests/` can construct providers
//! directly without going through the bootstrap binary.

pub mod llm;
pub mod prompts;
pub mod provider;
pub mod schemas;

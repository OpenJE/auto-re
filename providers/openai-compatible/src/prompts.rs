//! Prompt template registry backed by Handlebars.
//!
//! Templates are loaded from a directory at startup, one per capability,
//! plus `schema-repair.handlebars` for the bounded retry path. Each
//! capability has a fallback embedded so the provider can operate even
//! when the on-disk templates are missing — tests can override the
//! directory to inject minimal templates.

use std::collections::HashMap;
use std::path::Path;

use handlebars::Handlebars;

const CAPABILITY_IDS: &[&str] = &[
    "llm.analysis.function",
    "llm.analysis.type",
    "llm.analysis.class",
    "llm.analysis.subsystem",
    "llm.analysis.conflict",
    "llm.analysis.failure",
    "llm.experiment.design",
];

fn fallback(capability_id: &str) -> &'static str {
    match capability_id {
        "llm.analysis.function" => concat!(
            "Analyze the function with subject_entity_id {{subject_id}}.\n",
            "Return JSON conforming to the function-analysis response schema.\n",
            "Investigation bundle:\n{{{bundle}}}\n"
        ),
        "llm.analysis.type" => concat!(
            "Analyze the type with subject_entity_id {{subject_id}}.\n",
            "Return JSON conforming to the type-analysis response schema.\n",
            "Investigation bundle:\n{{{bundle}}}\n"
        ),
        "llm.analysis.class" => concat!(
            "Analyze the class with subject_entity_id {{subject_id}}.\n",
            "Return JSON conforming to the class-analysis response schema.\n",
            "Investigation bundle:\n{{{bundle}}}\n"
        ),
        "llm.analysis.subsystem" => concat!(
            "Identify the subsystem containing subject_entity_id {{subject_id}}.\n",
            "Return JSON conforming to the subsystem-analysis response schema.\n",
            "Investigation bundle:\n{{{bundle}}}\n"
        ),
        "llm.analysis.conflict" => concat!(
            "Resolve the conflict at subject_entity_id {{subject_id}}.\n",
            "Return JSON conforming to the conflict-analysis response schema.\n",
            "Investigation bundle:\n{{{bundle}}}\n"
        ),
        "llm.analysis.failure" => concat!(
            "Diagnose the failure rooted at subject_entity_id {{subject_id}}.\n",
            "Return JSON conforming to the failure-analysis response schema.\n",
            "Investigation bundle:\n{{{bundle}}}\n"
        ),
        "llm.experiment.design" => concat!(
            "Design an experiment for subject_entity_id {{subject_id}}.\n",
            "Return JSON conforming to the experiment-design response schema.\n",
            "Investigation bundle:\n{{{bundle}}}\n"
        ),
        _ => "Render the prompt for capability {{capability}}. Bundle:\n{{{bundle}}}\n",
    }
}

const SCHEMA_REPAIR_FALLBACK: &str = concat!(
    "The previous model output failed schema validation.\n",
    "Original bundle:\n{{{bundle}}}\n",
    "Previous invalid output:\n{{{invalid}}}\n",
    "Validation errors: {{{errors}}}\n",
    "Produce a new JSON response that conforms to the schema.\n"
);

/// Registry holding compiled Handlebars templates for each capability
/// and for the schema-repair retry path.
pub struct PromptRegistry {
    handlebars: Handlebars<'static>,
    capability_names: Vec<String>,
}

impl PromptRegistry {
    /// Load templates from `dir`. Missing files fall back to embedded defaults.
    pub fn load(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        let mut handlebars = Handlebars::new();
        handlebars.set_strict_mode(false);
        let mut capability_names = Vec::new();

        for id in CAPABILITY_IDS {
            let slug = id.replace('.', "_");
            let file = dir.join(format!("{slug}.handlebars"));
            let (source, origin) = match std::fs::read_to_string(&file) {
                Ok(s) => (s, "disk"),
                Err(_) => (fallback(id).to_string(), "fallback"),
            };
            if handlebars.register_template_string(id, source).is_ok() {
                capability_names.push((*id).to_string());
                tracing::debug!(capability = %id, origin, "registered prompt template");
            }
        }

        let repair_source = match std::fs::read_to_string(dir.join("schema_repair.handlebars")) {
            Ok(s) => s,
            Err(_) => SCHEMA_REPAIR_FALLBACK.to_string(),
        };
        let _ = handlebars.register_template_string("schema-repair", repair_source);

        Self {
            handlebars,
            capability_names,
        }
    }

    /// Capability IDs that have a registered template (disk or fallback).
    pub fn registered_capabilities(&self) -> &[String] {
        &self.capability_names
    }

    /// Render the prompt for the given capability using the investigation
    /// bundle as the template context.
    pub fn render(
        &self,
        capability_id: &str,
        bundle_json: &str,
    ) -> Result<String, handlebars::RenderError> {
        let subject_id = extract_subject_id(bundle_json).unwrap_or_else(|| "unknown".into());
        let mut ctx = HashMap::new();
        ctx.insert("bundle".to_string(), bundle_json.to_string());
        ctx.insert("subject_id".to_string(), subject_id);
        ctx.insert("capability".to_string(), capability_id.to_string());
        self.handlebars.render(capability_id, &ctx)
    }

    /// Render the schema-repair retry prompt with the failed bundle, the
    /// invalid output, and the validation error list as context.
    pub fn render_schema_repair(
        &self,
        bundle_json: &str,
        invalid_output: &str,
        validation_errors: &str,
    ) -> Result<String, handlebars::RenderError> {
        let mut ctx = HashMap::new();
        ctx.insert("bundle".to_string(), bundle_json.to_string());
        ctx.insert("invalid".to_string(), invalid_output.to_string());
        ctx.insert("errors".to_string(), validation_errors.to_string());
        self.handlebars.render("schema-repair", &ctx)
    }
}

fn extract_subject_id(bundle_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(bundle_json).ok()?;
    v.get("subject_entity_id")?.as_str().map(|s| s.to_string())
}

//! Builds a repair prompt from validation errors and context.

use handlebars::Handlebars;
use serde_json::json;

/// The embedded Handlebars template for schema-repair prompts.
const REPAIR_TEMPLATE: &str = include_str!("../../../prompts/schema-repair.handlebars");

/// Builds a repair prompt string that embeds the validation errors, the
/// expected response schema, and the original investigation bundle.
pub fn build_repair_prompt(
    capability_id: &str,
    validation_errors: &[String],
    response_schema_json: &str,
    bundle_json: &str,
) -> String {
    let mut hbs = Handlebars::new();
    hbs.set_strict_mode(false);
    hbs.register_template_string("repair", REPAIR_TEMPLATE)
        .expect("repair template is valid handlebars");

    let data = json!({
        "capability_id": capability_id,
        "validation_errors": validation_errors,
        "response_schema": response_schema_json,
        "bundle_json": bundle_json,
    });

    hbs.render("repair", &data)
        .unwrap_or_else(|e| format!("failed to render repair prompt: {e}"))
}

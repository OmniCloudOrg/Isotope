use anyhow::{Context, Result};
use handlebars::Handlebars;
use log::debug;
use serde_json::json;
use std::collections::HashMap;
use std::env;

/// Render a template with variables
pub fn render_template(template: &str, variables: &HashMap<String, String>) -> Result<String> {
    debug!("Rendering template with {} variables", variables.len());
    
    let mut handlebars = Handlebars::new();
    handlebars.set_strict_mode(true);
    
    // Register the template
    handlebars.register_template_string("template", template)
        .context("Failed to register template")?;
    
    // Create a JSON object from the variables
    let mut data = serde_json::Map::new();
    
    // Add environment variables
    let mut env_vars = serde_json::Map::new();
    for (key, value) in env::vars() {
        env_vars.insert(key, json!(value));
    }
    data.insert("env".to_string(), json!(env_vars));
    
    // Add template variables
    for (key, value) in variables {
        data.insert(key.clone(), json!(value));
    }
    
    // Render the template
    let rendered = handlebars.render("template", &json!(data))
        .context("Failed to render template")?;
    
    debug!("Template rendered successfully");
    Ok(rendered)
}

/// Process and substitute variables in a template file
pub fn process_templates<T: AsRef<str>>(content: T, variables: &HashMap<String, String>) -> Result<String> {
    let content = content.as_ref();
    
    // This is a simplified implementation that just replaces {{ var }} patterns
    // In a real implementation, we would use a proper template engine like handlebars
    
    let mut result = content.to_string();
    
    for (key, value) in variables {
        let pattern = format!("{{{{ {} }}}}", key);
        result = result.replace(&pattern, value);
        
        // Also handle environment variable references
        let env_pattern = format!("{{{{ env.{} }}}}", key);
        if let Ok(env_value) = env::var(key) {
            result = result.replace(&env_pattern, &env_value);
        }
    }
    
    Ok(result)
}
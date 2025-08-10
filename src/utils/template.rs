use anyhow::{Context, Result};
use handlebars::Handlebars;
use std::collections::HashMap;
use tracing::debug;

pub struct TemplateEngine {
    handlebars: Handlebars<'static>,
}

impl TemplateEngine {
    pub fn new() -> Self {
        let mut handlebars = Handlebars::new();

        // Configure handlebars settings
        handlebars.set_strict_mode(false); // Allow undefined variables

        Self { handlebars }
    }

    pub fn render_string(
        &self,
        template: &str,
        variables: &HashMap<String, String>,
    ) -> Result<String> {
        debug!("Rendering template: {}", template);

        // Convert environment variable format ${VAR} to handlebars format {{VAR}}
        let handlebars_template = self.convert_env_vars_to_handlebars(template);

        self.handlebars
            .render_template(&handlebars_template, variables)
            .with_context(|| format!("Failed to render template: {template}"))
    }

    pub fn render_file(
        &self,
        template_path: &str,
        output_path: &str,
        variables: &HashMap<String, String>,
    ) -> Result<()> {
        debug!(
            "Rendering template file: {} -> {}",
            template_path, output_path
        );

        let template_content = std::fs::read_to_string(template_path)
            .with_context(|| format!("Failed to read template file: {template_path}"))?;

        let rendered = self.render_string(&template_content, variables)?;

        std::fs::write(output_path, rendered)
            .with_context(|| format!("Failed to write rendered template to: {output_path}"))?;

        Ok(())
    }

    pub fn register_template(&mut self, name: &str, template: &str) -> Result<()> {
        self.handlebars
            .register_template_string(name, template)
            .with_context(|| format!("Failed to register template: {name}"))
    }

    pub fn register_template_file(&mut self, name: &str, template_path: &str) -> Result<()> {
        self.handlebars
            .register_template_file(name, template_path)
            .with_context(|| format!("Failed to register template file: {template_path}"))
    }

    pub fn render_registered(
        &self,
        template_name: &str,
        variables: &HashMap<String, String>,
    ) -> Result<String> {
        self.handlebars
            .render(template_name, variables)
            .with_context(|| format!("Failed to render registered template: {template_name}"))
    }

    fn convert_env_vars_to_handlebars(&self, template: &str) -> String {
        // Convert ${VAR_NAME} to {{VAR_NAME}}
        // This handles both ${VAR} and ${env.VAR} formats
        let mut result = template.to_string();

        // Handle ${env.VAR_NAME} format (common in configs)
        result = regex::Regex::new(r"\$\{env\.([^}]+)\}")
            .unwrap()
            .replace_all(&result, "{{$1}}")
            .to_string();

        // Handle ${VAR_NAME} format
        result = regex::Regex::new(r"\$\{([^}]+)\}")
            .unwrap()
            .replace_all(&result, "{{$1}}")
            .to_string();

        result
    }

    pub fn add_helper<F>(&mut self, name: &str, helper: F)
    where
        F: handlebars::HelperDef + Send + Sync + 'static,
    {
        self.handlebars.register_helper(name, Box::new(helper));
    }

    pub fn create_context_from_env() -> HashMap<String, String> {
        std::env::vars().collect()
    }

    pub fn merge_contexts(
        base: &HashMap<String, String>,
        overlay: &HashMap<String, String>,
    ) -> HashMap<String, String> {
        let mut merged = base.clone();
        for (key, value) in overlay {
            merged.insert(key.clone(), value.clone());
        }
        merged
    }

    pub fn validate_template(&self, template: &str) -> Result<()> {
        // Try to compile the template to check for syntax errors
        self.handlebars
            .render_template(template, &HashMap::<String, String>::new())
            .map(|_| ())
            .or_else(|e| {
                // If it fails due to missing variables, that's OK for validation
                if e.to_string().contains("variable") {
                    Ok(())
                } else {
                    Err(e)
                }
            })
            .with_context(|| format!("Template validation failed: {template}"))
    }
}

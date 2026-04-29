//! Tool: `file.read_safe` — lee archivos respetando paths sensibles.
//!
//! Antes de devolver el contenido al agente, comprueba contra el módulo
//! `ironclaw_safety::sensitive_paths`. Si el path coincide con un patrón
//! sensible (ssh keys, browser history, password stores) → devuelve
//! contenido REDACTADO con metadata "matched: <pattern>" para que el
//! agente sepa qué pasó sin ver el contenido real.
//!
//! Categoría: ReadSensitive estática para que el guardian fuerce CONFIRM
//! incluso para reads. v12 puede splitear en tools separadas si conviene.

use crate::{
    error::{Error, Result},
    tool::{Tool, ToolMetadata, ToolOutput},
};
use jarvis_policies::ActionCategory;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct Args {
    path: String,
    /// Tamaño máximo a leer en bytes. Default 64 KB.
    #[serde(default = "default_max_bytes")]
    max_bytes: usize,
}

fn default_max_bytes() -> usize {
    65536
}

pub struct FileReadSafeTool {
    metadata: ToolMetadata,
}

impl FileReadSafeTool {
    pub fn new() -> Self {
        let metadata = ToolMetadata {
            name: "file.read_safe".to_string(),
            description: "Read a file from disk with sensitive-path redaction. \
                          If the path matches patterns from ironclaw_safety::sensitive_paths \
                          (ssh keys, browser history, password stores, .env files, etc.), \
                          the content is replaced with `[REDACTED]` and only metadata \
                          is returned. Otherwise, returns content (truncated to max_bytes \
                          if too large)."
                .to_string(),
            category: ActionCategory::ReadSensitive,
            args_schema: serde_json::json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute path to file to read"
                    },
                    "max_bytes": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 1048576,
                        "default": 65536,
                        "description": "Max bytes to read (file truncated if larger)"
                    }
                }
            }),
        };
        Self { metadata }
    }
}

impl Default for FileReadSafeTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Tool for FileReadSafeTool {
    fn metadata(&self) -> &ToolMetadata {
        &self.metadata
    }

    async fn invoke(&self, args: &serde_json::Value) -> Result<ToolOutput> {
        let parsed: Args = serde_json::from_value(args.clone())
            .map_err(|e| Error::InvalidArguments(format!("file.read_safe args: {e}")))?;

        let path = Path::new(&parsed.path);

        // Comprueba contra patrones sensibles ANTES de leer.
        let is_sensitive = ironclaw_safety::sensitive_paths::is_sensitive_path(path);

        if is_sensitive {
            let metadata = std::fs::metadata(path).ok();
            let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
            return Ok(ToolOutput::new(serde_json::json!({
                "path": parsed.path.clone(),
                "redacted": true,
                "reason": "sensitive_path_match",
                "size_bytes": size,
                "content": "[REDACTED: matched ironclaw_safety::sensitive_paths]",
            }))
            .with_user_message(format!("REDACTED (sensitive path): {}", parsed.path)));
        }

        let content = tokio::fs::read(path).await.map_err(Error::Io)?;

        let total_size = content.len();
        let truncated = total_size > parsed.max_bytes;
        let to_send = if truncated {
            &content[..parsed.max_bytes]
        } else {
            &content[..]
        };

        let (content_str, is_text) = match std::str::from_utf8(to_send) {
            Ok(s) => (s.to_string(), true),
            Err(_) => (
                format!("[binary content, {} bytes]", to_send.len()),
                false,
            ),
        };

        Ok(ToolOutput::new(serde_json::json!({
            "path": parsed.path.clone(),
            "redacted": false,
            "size_bytes": total_size,
            "truncated": truncated,
            "is_text": is_text,
            "content": content_str,
        }))
        .with_user_message(format!(
            "read {} bytes{}",
            to_send.len(),
            if truncated { " (truncated)" } else { "" }
        )))
    }
}

//! AI DBA Copilot — LLM backend with dynamic schema injection, query explanation and index
//! suggestions.
//!
//! Two backends:
//! 1. 9Router (default) — OpenAI-compatible at `http://localhost:20128/v1`.
//! 2. Ollama — at `http://127.0.0.1:11434`.
//!
//! The schema context is fetched via `QueryExecutor::list_tables()` and injected into the
//! LLM system prompt so the model generates SQL that matches the live database schema.

use crate::connection::ConnectionConfig;
use crate::error::CatableError;
use crate::executor::{QueryExecutor, TableInfo};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Backend configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiBackendConfig {
    /// Base URL (OpenAI-compatible or Ollama).
    pub base_url: String,
    /// Optional API key.
    pub api_key: Option<String>,
    /// Model name.
    pub model: String,
    /// Maximum tokens to generate.
    pub max_tokens: u32,
    /// Sampling temperature.
    pub temperature: f32,
}

impl Default for AiBackendConfig {
    fn default() -> Self {
        Self::nine_router()
    }
}

impl AiBackendConfig {
    /// 9Router local endpoint — the default for CATable.
    pub fn nine_router() -> Self {
        Self {
            base_url: "http://localhost:20128/v1".into(),
            api_key: Some("sk_9router".into()),
            model: "ZA126_PRO".into(),
            max_tokens: 8192,
            temperature: 0.2,
        }
    }

    /// Ollama local endpoint.
    pub fn ollama() -> Self {
        Self {
            base_url: "http://127.0.0.1:11434".into(),
            api_key: None,
            model: "llama3.1:8b".into(),
            max_tokens: 8192,
            temperature: 0.2,
        }
    }
}

// ---------------------------------------------------------------------------
// Schema injection
// ---------------------------------------------------------------------------

/// The schema for one active database connection, used to build the LLM system prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaContext {
    /// Human-readable driver label, e.g. `sqlite`, `postgres`.
    pub driver: String,
    /// Database name / path label.
    pub database: String,
    /// Row-count + column info for every table.
    pub tables: Vec<TableInfo>,
}

impl SchemaContext {
    /// Builds the schema context for an active connection by calling `list_tables`.
    pub fn from_connection(conn: &ConnectionConfig) -> Result<Self, CatableError> {
        let tables = QueryExecutor::list_tables(conn)?;
        let driver = match conn.driver {
            crate::connection::DriverKind::Sqlite => "SQLite".to_string(),
            crate::connection::DriverKind::Postgres => "PostgreSQL".to_string(),
            crate::connection::DriverKind::Mysql => "MySQL".to_string(),
            crate::connection::DriverKind::MariaDb => "MariaDB".to_string(),
            crate::connection::DriverKind::DuckDb => "DuckDB".to_string(),
            crate::connection::DriverKind::Redis => "Redis".to_string(),
        };
        Ok(Self {
            driver,
            database: conn.name.clone(),
            tables,
        })
    }

    /// Renders the schema as a compact SQL-annotated string for the system prompt.
    pub fn to_prompt(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "You are a database expert assistant for a {} database.\n",
            self.driver
        ));
        out.push_str("The live schema is as follows:\n\n");
        for table in &self.tables {
            out.push_str(&format!(
                "TABLE {} (~{} rows):\n",
                table.name, table.row_count
            ));
            for col in &table.columns {
                let mut spec = format!("  {} {}", col.name, col.data_type);
                if col.is_pk {
                    spec.push_str(" [PK]");
                }
                if col.not_null {
                    spec.push_str(" [NOT NULL]");
                }
                out.push_str(&spec);
                out.push('\n');
            }
            out.push('\n');
        }
        out.push_str(
            "Only reference tables and columns that appear above. Generate valid SQL for \
             this dialect. Always wrap SQL in a fenced code block with ```sql ... ```.\n",
        );
        out
    }
}

// ---------------------------------------------------------------------------
// LLM wire format
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LlmRequest {
    model: String,
    messages: Vec<LlmMessage>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LlmMessage {
    role: String,
    content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LlmResponse {
    choices: Vec<LlmChoice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LlmChoice {
    message: LlmMessage,
}

fn llm_request(
    backend: &AiBackendConfig,
    system_prompt: &str,
    user_prompt: &str,
) -> LlmRequest {
    LlmRequest {
        model: backend.model.clone(),
        messages: vec![
            LlmMessage {
                role: "system".into(),
                content: system_prompt.to_string(),
            },
            LlmMessage {
                role: "user".into(),
                content: user_prompt.to_string(),
            },
        ],
        max_tokens: backend.max_tokens,
        temperature: backend.temperature,
    }
}

/// Calls the LLM and returns the raw assistant text.
fn call_llm(backend: &AiBackendConfig, req: &LlmRequest) -> Result<String, CatableError> {
    let base = backend.base_url.trim_end_matches('/');
    let is_ollama = !base.ends_with("/v1");

    let (url, body) = if is_ollama {
        // Ollama uses its own native endpoint
        (
            format!("{}/api/chat", base),
            serde_json::json!({
                "model": req.model,
                "messages": req.messages,
                "stream": false,
                "options": { "temperature": req.temperature, "num_predict": req.max_tokens as i64 }
            }),
        )
    } else {
        // OpenAI-compatible (9Router, any /v1 proxy)
        (
            format!("{}/chat/completions", base),
            serde_json::to_value(req).map_err(|e| {
                CatableError::Driver {
                    driver: "ai".into(),
                    message: format!("JSON encode failed: {e}"),
                }
            })?,
        )
    };

    let body_str = body.to_string();

    let req_result = ureq::post(&url)
        .timeout(std::time::Duration::from_secs(120))
        .header("Content-Type", "application/json")
        .set(
            "Authorization",
            &format!(
                "Bearer {}",
                backend.api_key.clone().unwrap_or_else(|| "no-key".into())
            ),
        )
        .call();

    let response = match req_result {
        Ok(r) => r,
        Err(ureq::Error::Transport(_)) => {
            // Ollama sometimes ignores Bearer header — retry without auth
            ureq::post(&url)
                .timeout(std::time::Duration::from_secs(120))
                .header("Content-Type", "application/json")
                .call()
                .map_err(|e| CatableError::Driver {
                    driver: "ai".into(),
                    message: format!("LLM endpoint unreachable: {e}"),
                })?
        }
        Err(ureq::Error::Status(code, r)) => {
            let text = r.into_string().unwrap_or_default();
            return Err(CatableError::Driver {
                driver: "ai".into(),
                message: format!("LLM HTTP {code}: {text}"),
            });
        }
    };

    let text: String = response
        .into_string()
        .map_err(|e| {
            CatableError::Driver {
                driver: "ai".into(),
                message: format!("Failed to read LLM response body: {e}"),
            }
        })?;

    // Ollama returns `message.content`; OpenAI returns `choices[0].message.content`
    let val: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        CatableError::Driver {
            driver: "ai".into(),
            message: format!("Invalid JSON from LLM: {e}"),
        }
    })?;

    let content = val
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .or_else(|| {
            val.get("message")
                .and_then(|m| m.get("content"))
        })
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            CatableError::Driver {
                driver: "ai".into(),
                message: format!("Unexpected LLM response shape: {text}"),
            }
        })?
        .to_string();

    Ok(content)
}

// ---------------------------------------------------------------------------
// Copilot handlers
// ---------------------------------------------------------------------------

/// Full natural-language request: prompt + active schema → SQL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiChatRequest {
    pub prompt: String,
    pub schema: Option<SchemaContext>,
}

/// A completed AI chat turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiChatResponse {
    /// The LLM's full text response (explanation + SQL block).
    pub reply: String,
    /// Extracted SQL (if found inside a fenced block).
    pub sql: Option<String>,
}

/// Request to explain a query and suggest indexes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiExplainRequest {
    pub sql: String,
    pub schema: Option<SchemaContext>,
}

/// Result of `explain_query` / `suggest_indexes`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiExplainResponse {
    /// Plain-language explanation of what the SQL does.
    pub explanation: String,
    /// List of recommended `CREATE INDEX` statements.
    pub index_suggestions: Vec<String>,
}

pub struct AiDbaCopilot;

impl AiDbaCopilot {
    /// Main chat handler: sends the user's natural-language prompt plus live schema to the
    /// LLM and returns the response + extracted SQL.
    pub fn chat(
        backend: &AiBackendConfig,
        req: &AiChatRequest,
    ) -> Result<AiChatResponse, CatableError> {
        let system = match &req.schema {
            Some(schema) => {
                let mut s = schema.to_prompt();
                s.push_str(
                    "\nWhen the user asks for a SQL query, respond with:\n",
                );
                s.push_str("1. A short explanation (2-4 sentences)\n");
                s.push_str("2. The SQL in a ```sql fenced code block\n");
                s
            }
            None => "You are a database expert. Explain your SQL in 2-4 sentences, then show it \
                     in a ```sql fenced code block. Use only tables and columns the user \
                     mentions, or ask for clarification if the schema is unknown."
                .to_string(),
        };

        let user = format!(
            "Task: {}\n\nGenerate the SQL query for this request.",
            req.prompt
        );

        let raw = call_llm(backend, &llm_request(backend, &system, &user))?;
        let sql = extract_sql_block(&raw);
        Ok(AiChatResponse {
            reply: raw,
            sql,
        })
    }

    /// Explains an existing SQL query and suggests indexes that would speed it up.
    pub fn explain(
        backend: &AiBackendConfig,
        req: &AiExplainRequest,
    ) -> Result<AiExplainResponse, CatableError> {
        let schema_block = req
            .schema
            .as_ref()
            .map(|s| s.to_prompt())
            .unwrap_or_else(|| "No live schema available.".to_string());

        let system = format!(
            "You are a performance-focused DBA. Given the SQL below and the live schema, \
             explain what the query does, identify slow paths, and recommend specific \
             CREATE INDEX statements that would improve it.\n\nSchema:\n{schema_block}\n\n\
             Respond with:\n1. **Explanation** — 2-5 sentences\n2. **Index suggestions** — \
             a bulleted list of CREATE INDEX statements (or 'No index changes needed')."
        );

        let user = format!("SQL to explain:\n```\n{}\n```", req.sql);

        let raw = call_llm(backend, &llm_request(backend, &system, &user))?;

        let explanation = raw
            .split("**Index")
            .next()
            .unwrap_or(&raw)
            .trim()
            .to_string();

        let index_suggestions: Vec<String> = raw
            .lines()
            .filter(|l| l.trim().to_uppercase().starts_with("CREATE INDEX"))
            .map(|l| l.trim().trim_start_matches("•").trim().to_string())
            .collect();

        Ok(AiExplainResponse {
            explanation,
            index_suggestions,
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extracts the first ```sql fenced block from LLM output.
pub fn extract_sql_block(text: &str) -> Option<String> {
    let start = text.find("```sql")?;
    let after_lang = &text[start + 6..]; // skip "```sql"
    let end = after_lang.rfind("```")?;
    Some(after_lang[..end].trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_sql_block_basic() {
        let text = "Here you go:\n```sql\nSELECT 1\n```\ndone";
        assert_eq!(extract_sql_block(text), Some("SELECT 1".to_string()));
    }

    #[test]
    fn extract_sql_block_missing() {
        assert_eq!(extract_sql_block("no sql here"), None);
    }

    #[test]
    fn nine_router_defaults() {
        let cfg = AiBackendConfig::nine_router();
        assert!(cfg.base_url.contains("20128"));
        assert_eq!(cfg.model, "ZA126_PRO");
    }

    #[test]
    fn ollama_defaults() {
        let cfg = AiBackendConfig::ollama();
        assert!(cfg.base_url.contains("11434"));
    }

    #[test]
    fn schema_context_prompt_includes_table_names() {
        let tables = vec![TableInfo {
            name: "users".into(),
            table_type: "table".into(),
            columns: vec![
                crate::executor::TableColumnInfo {
                    name: "id".into(),
                    data_type: "INTEGER".into(),
                    is_pk: true,
                    not_null: true,
                },
                crate::executor::TableColumnInfo {
                    name: "email".into(),
                    data_type: "TEXT".into(),
                    is_pk: false,
                    not_null: true,
                },
            ],
            row_count: 1000,
        }];
        let ctx = SchemaContext {
            driver: "SQLite".into(),
            database: "test".into(),
            tables,
        };
        let prompt = ctx.to_prompt();
        assert!(prompt.contains("users"));
        assert!(prompt.contains("email"));
        assert!(prompt.contains("SQLite"));
    }
}

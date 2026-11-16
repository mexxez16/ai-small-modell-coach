//! Chat provider configuration: which AI backends the chat dock can connect
//! to. Supports any OpenAI-compatible endpoint (Mistral API, Ollama, LM
//! Studio, llama.cpp server, vLLM, ...) and the Mistral Vibe CLI.
//!
//! Providers are stored in `<data_dir>/providers.json` so they survive
//! restarts. API keys are never returned by the REST API in cleartext — the
//! list endpoint masks them.

use serde::{Deserialize, Serialize};
use std::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// OpenAI-compatible chat API (`{base_url}/chat/completions`).
    #[serde(rename = "openai_compatible", alias = "open_ai_compatible")]
    OpenAiCompatible,
    /// Mistral Vibe CLI, spawned as a PTY scoped to the workspace dir.
    Vibe,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    /// OpenAI-compatible only: e.g. `http://localhost:11434/v1` (Ollama),
    /// `https://api.mistral.ai/v1` (Mistral), `http://localhost:1234/v1`.
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

impl Provider {
    /// Cleartext-safe view for the REST API.
    pub fn masked(&self) -> Provider {
        let mut out = self.clone();
        if let Some(key) = &out.api_key {
            let prefix: String = key.chars().take(3).collect();
            out.api_key = Some(format!("{prefix}…"));
        }
        out
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ProviderFile {
    providers: Vec<Provider>,
}

pub struct ProviderStore {
    path: std::path::PathBuf,
    inner: RwLock<ProviderFile>,
}

impl ProviderStore {
    pub fn load(data_dir: &std::path::Path) -> Self {
        let path = data_dir.join("providers.json");
        let providers = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<ProviderFile>(&bytes)
                .map(|f| f.providers)
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        ProviderStore {
            path,
            inner: RwLock::new(ProviderFile { providers }),
        }
    }

    pub fn list(&self, mask_keys: bool) -> Vec<Provider> {
        self.inner
            .read()
            .unwrap()
            .providers
            .iter()
            .map(|p| if mask_keys { p.masked() } else { p.clone() })
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<Provider> {
        self.inner
            .read()
            .unwrap()
            .providers
            .iter()
            .find(|p| p.id == id)
            .cloned()
    }

    pub fn upsert(&self, provider: Provider) -> Provider {
        let provider_id = provider.id.clone();
        let mut file = self.inner.write().unwrap();
        if let Some(existing) = file.providers.iter_mut().find(|p| p.id == provider_id) {
            // Keep the real key when the update sends a masked/empty one.
            let key = match provider.api_key.as_deref() {
                Some(k) if !k.is_empty() && k.ends_with('…') || (k.len() == 4 && k.ends_with('…')) => {
                    existing.api_key.clone()
                }
                Some(k) if !k.is_empty() => Some(k.to_string()),
                _ => existing.api_key.clone(),
            };
            *existing = provider;
            existing.api_key = key;
        } else {
            file.providers.push(provider.clone());
        }
        self.persist(&file);
        file.providers
            .iter()
            .find(|p| p.id == provider_id)
            .cloned()
            .expect("provider just inserted/updated")
    }

    pub fn delete(&self, id: &str) -> bool {
        let mut file = self.inner.write().unwrap();
        let before = file.providers.len();
        file.providers.retain(|p| p.id != id);
        let removed = file.providers.len() != before;
        if removed {
            self.persist(&file);
        }
        removed
    }

    fn persist(&self, file: &ProviderFile) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = self.path.with_extension("json.tmp");
        if serde_json::to_writer_pretty(std::fs::File::create(&tmp).expect("create tmp config"), file).is_ok() {
            let _ = std::fs::rename(&tmp, &self.path);
        }
    }
}

pub fn new_provider(
    name: String,
    kind: ProviderKind,
    base_url: Option<String>,
    api_key: Option<String>,
    model: Option<String>,
) -> Provider {
    Provider {
        id: coach_core::model::new_id(),
        name,
        kind,
        base_url,
        api_key,
        model,
    }
}

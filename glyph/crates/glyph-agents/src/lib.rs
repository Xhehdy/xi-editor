//! Glyph Agents - WASM-based agent runtime
//!
//! Provides sandboxed execution environment for AI code agents.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{info, warn};
use wasmtime::*;
use wasmtime_wasi::preview1;
use wasmtime_wasi::WasiCtxBuilder;

/// Agent runtime errors
#[derive(Debug, Error)]
pub enum AgentError {
    #[error("Failed to load agent: {0}")]
    LoadFailed(String),
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Agent not found: {0}")]
    NotFound(String),
    #[error("Invalid agent manifest")]
    InvalidManifest,
}

/// Unique agent identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(pub String);

/// Agent capabilities (sandboxing permissions)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentCapabilities {
    /// Can read files
    pub read_files: bool,
    /// Can write files
    pub write_files: bool,
    /// Can make network requests
    pub network: bool,
    /// Can execute subprocesses
    pub subprocess: bool,
    /// Memory limit in bytes
    pub memory_limit: Option<usize>,
    /// CPU time limit in milliseconds
    pub cpu_limit_ms: Option<u64>,
}

/// Agent manifest (describes the agent)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentManifest {
    pub id: AgentId,
    pub name: String,
    pub version: String,
    pub description: String,
    pub capabilities: AgentCapabilities,
    /// Entry point function name
    pub entry_point: String,
}

/// The agent runtime
pub struct AgentRuntime {
    engine: Engine,
    agents: Arc<RwLock<HashMap<AgentId, Arc<AgentManifest>>>>,
}

impl AgentRuntime {
    /// Creates a new agent runtime
    pub fn new() -> Result<Self, AgentError> {
        let mut config = Config::new();
        config.async_support(true);
        config.consume_fuel(true); // Enable fuel for CPU limiting

        let engine = Engine::new(&config).map_err(|e| AgentError::LoadFailed(e.to_string()))?;

        Ok(Self {
            engine,
            agents: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Loads an agent from WASM bytes
    pub async fn load_agent(
        &self,
        manifest: AgentManifest,
        wasm_bytes: &[u8],
    ) -> Result<AgentId, AgentError> {
        if manifest.entry_point.trim().is_empty() {
            return Err(AgentError::InvalidManifest);
        }

        Module::new(&self.engine, wasm_bytes).map_err(|e| AgentError::LoadFailed(e.to_string()))?;

        let id = manifest.id.clone();

        // Store the manifest
        {
            let mut agents = self.agents.write().await;
            agents.insert(id.clone(), Arc::new(manifest));
        }

        info!("Loaded agent: {:?}", id);
        Ok(id)
    }

    /// Executes an agent with given input
    pub async fn execute(
        &self,
        agent_id: &AgentId,
        wasm_bytes: &[u8],
        _input: &str,
    ) -> Result<String, AgentError> {
        let manifest = {
            let agents = self.agents.read().await;
            agents
                .get(agent_id)
                .cloned()
                .ok_or_else(|| AgentError::NotFound(agent_id.0.clone()))?
        };

        if manifest.capabilities.read_files
            || manifest.capabilities.write_files
            || manifest.capabilities.network
            || manifest.capabilities.subprocess
        {
            warn!(
                "Agent {} requested unsupported capabilities; rejecting execution",
                manifest.name
            );
            return Err(AgentError::ExecutionFailed(
                "Requested capabilities are not yet supported by Glyph sandbox".to_string(),
            ));
        }

        // Create a minimal WASI context with no inherited IO.
        let wasi = WasiCtxBuilder::new().build_p1();

        let mut store = Store::new(&self.engine, wasi);

        // Set fuel limit if specified
        if let Some(limit) = manifest.capabilities.cpu_limit_ms {
            store
                .set_fuel(limit.saturating_mul(1000))
                .map_err(|e| AgentError::ExecutionFailed(e.to_string()))?;
        }

        // Compile and instantiate
        let module = Module::new(&self.engine, wasm_bytes)
            .map_err(|e| AgentError::ExecutionFailed(e.to_string()))?;

        let mut linker = Linker::new(&self.engine);
        preview1::add_to_linker_sync(&mut linker, |ctx| ctx)
            .map_err(|e| AgentError::ExecutionFailed(e.to_string()))?;

        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| AgentError::ExecutionFailed(e.to_string()))?;

        // Call the declared zero-arg entrypoint.
        let entry = instance
            .get_typed_func::<(), ()>(&mut store, &manifest.entry_point)
            .map_err(|e| AgentError::ExecutionFailed(e.to_string()))?;
        entry
            .call(&mut store, ())
            .map_err(|e| AgentError::ExecutionFailed(e.to_string()))?;

        info!("Executed agent {:?}", agent_id);

        Ok(format!("Agent {} executed successfully", manifest.name))
    }

    /// Lists all loaded agents
    pub async fn list_agents(&self) -> Vec<AgentManifest> {
        let agents = self.agents.read().await;
        agents.values().map(|a| (**a).clone()).collect()
    }

    /// Unloads an agent
    pub async fn unload_agent(&self, agent_id: &AgentId) -> Result<(), AgentError> {
        let mut agents = self.agents.write().await;
        agents
            .remove(agent_id)
            .ok_or_else(|| AgentError::NotFound(agent_id.0.clone()))?;
        info!("Unloaded agent: {:?}", agent_id);
        Ok(())
    }
}

impl Default for AgentRuntime {
    fn default() -> Self {
        Self::new().expect("Failed to create agent runtime")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_runtime_creation() {
        let runtime = AgentRuntime::new().unwrap();
        let agents = runtime.list_agents().await;
        assert!(agents.is_empty());
    }
}

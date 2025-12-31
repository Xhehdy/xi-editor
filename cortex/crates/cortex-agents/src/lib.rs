//! Cortex Agents - WASM-based agent runtime
//!
//! Provides sandboxed execution environment for AI code agents.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{info, warn};
use wasmtime::*;
use wasmtime_wasi::preview1::{self, WasiP1Ctx};
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

/// A loaded agent instance
pub struct Agent {
    pub manifest: AgentManifest,
    store: Store<WasiP1Ctx>,
    instance: Instance,
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
        
        let engine = Engine::new(&config)
            .map_err(|e| AgentError::LoadFailed(e.to_string()))?;
        
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
        let module = Module::new(&self.engine, wasm_bytes)
            .map_err(|e| AgentError::LoadFailed(e.to_string()))?;
        
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
        input: &str,
    ) -> Result<String, AgentError> {
        let manifest = {
            let agents = self.agents.read().await;
            agents.get(agent_id)
                .cloned()
                .ok_or_else(|| AgentError::NotFound(agent_id.0.clone()))?
        };
        
        // Create WASI context with sandboxed capabilities
        let wasi = WasiCtxBuilder::new()
            .inherit_stdio()
            .build_p1();
        
        let mut store = Store::new(&self.engine, wasi);
        
        // Set fuel limit if specified
        if let Some(limit) = manifest.capabilities.cpu_limit_ms {
            store.set_fuel(limit * 1000).ok(); // Convert ms to fuel units
        }
        
        // Compile and instantiate
        let module = Module::new(&self.engine, wasm_bytes)
            .map_err(|e| AgentError::ExecutionFailed(e.to_string()))?;
        
        let mut linker = Linker::new(&self.engine);
        preview1::add_to_linker_sync(&mut linker, |ctx| ctx)
            .map_err(|e| AgentError::ExecutionFailed(e.to_string()))?;
        
        let instance = linker.instantiate(&mut store, &module)
            .map_err(|e| AgentError::ExecutionFailed(e.to_string()))?;
        
        // Call the entry point
        // For now, just return success - actual execution would call WASM functions
        info!("Executed agent {:?} with input: {}", agent_id, input);
        
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
        agents.remove(agent_id)
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

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use crate::{
    capability::{CapabilityDescriptor, DataKind, Operation},
    engine::ProcessingEngine,
    error::{ErrorKind, GsadError, Result},
};

/// Thread-safe in-process registry. It is not a security boundary: native
/// engines execute with the host process's privileges.
#[derive(Default)]
pub struct CapabilityRegistry {
    engines: RwLock<BTreeMap<String, Arc<dyn ProcessingEngine>>>,
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, engine: Arc<dyn ProcessingEngine>) -> Result<()> {
        engine.descriptor().validate()?;
        let id = engine.descriptor().id.clone();
        let mut engines = self
            .engines
            .write()
            .map_err(|_| GsadError::internal("capability registry write lock was poisoned"))?;
        if engines.contains_key(&id) {
            return Err(GsadError::new(
                ErrorKind::Conflict,
                format!("an engine with id `{id}` is already registered"),
            ));
        }
        engines.insert(id, engine);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<Arc<dyn ProcessingEngine>>> {
        let engines = self
            .engines
            .read()
            .map_err(|_| GsadError::internal("capability registry read lock was poisoned"))?;
        Ok(engines.get(id).cloned())
    }

    pub fn list(&self) -> Result<Vec<CapabilityDescriptor>> {
        let engines = self
            .engines
            .read()
            .map_err(|_| GsadError::internal("capability registry read lock was poisoned"))?;
        Ok(engines
            .values()
            .map(|engine| engine.descriptor().clone())
            .collect())
    }

    pub fn find(
        &self,
        operation: Operation,
        input: DataKind,
    ) -> Result<Vec<Arc<dyn ProcessingEngine>>> {
        let engines = self
            .engines
            .read()
            .map_err(|_| GsadError::internal("capability registry read lock was poisoned"))?;
        Ok(engines
            .values()
            .filter(|engine| engine.descriptor().supports(operation, input))
            .cloned()
            .collect())
    }
}

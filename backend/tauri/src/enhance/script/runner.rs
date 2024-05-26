use anyhow::Error;
use serde_yaml::Mapping;

pub trait RunnerManager {
    fn try_new() -> Result<Self, Error>
    where
        Self: std::marker::Sized;
    /// Process profiles by script file path
    async fn process(&self, mapping: Mapping, path: &str) -> Result<Mapping, Error>;
    
    /// Honey replacement - use in memory code str to load module and exec it!
    /// It might not be implemented - due to some embeded engine is not support. 
    async fn process_honey(&self, mapping: Mapping, script: &str) -> Result<Mapping, Error> {
        tracing::debug!("mapping: {:?}\nscript:{}", mapping, script);
        unimplemented!()
    }
}

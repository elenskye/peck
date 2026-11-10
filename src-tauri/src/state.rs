use crate::run::ActiveRunContext;
use crate::store::Store;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct AppState {
    pub api: Arc<crate::api::ApiState>,
    pub store: Arc<Mutex<Store>>,
    pub active_runs: Arc<Mutex<HashMap<String, ActiveRunContext>>>,
    pub usage_monitor: Arc<crate::usage::UsageMonitor>,
}

impl AppState {
    pub fn new(store: Store) -> crate::store::StoreResult<Self> {
        let mode = crate::mode::api_mode(store.api_mode()?);
        Ok(Self {
            api: Arc::new(crate::api::ApiState::new(mode)),
            store: Arc::new(Mutex::new(store)),
            active_runs: Arc::new(Mutex::new(HashMap::new())),
            usage_monitor: Arc::new(crate::usage::UsageMonitor::new()),
        })
    }

}

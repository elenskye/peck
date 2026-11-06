use crate::run::ActiveRunContext;
use crate::store::Store;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct AppState {
    pub store: Arc<Mutex<Store>>,
    pub active_runs: Arc<Mutex<HashMap<String, ActiveRunContext>>>,
}

impl AppState {
    pub fn new(store: Store) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
            active_runs: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

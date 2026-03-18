use log::debug;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct UserLocks {
    locks: Arc<Mutex<HashMap<i64, Arc<Mutex<()>>>>>,
}

impl UserLocks {
    pub fn new() -> Self {
        Self {
            locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn acquire(&self, chat_id: i64) -> Arc<Mutex<()>> {
        let lock = {
            let mut locks = self.locks.lock().await;
            locks
                .entry(chat_id)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };

        debug!("Acquired lock for user {}", chat_id);
        lock
    }
}

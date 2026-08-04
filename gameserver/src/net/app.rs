use sqlx::SqlitePool;
use tokio::sync::{Mutex, mpsc};

use crate::net::outbound::CommandPacket;

/// App-level shared state
pub struct AppState {
    next_down_tag: Mutex<u8>,
    pub db: &'static SqlitePool,
    pub tables: &'static config::GameDB,
    sessions: dashmap::DashMap<i64, mpsc::Sender<CommandPacket>>,
}

#[allow(dead_code)]
impl AppState {
    pub fn new(db: SqlitePool, tables: &'static config::GameDB) -> Self {
        Self {
            next_down_tag: Mutex::new(0),
            db: Box::leak(Box::new(db)),
            tables,
            sessions: dashmap::DashMap::new(),
        }
    }

    pub async fn reserve_down_tag(&self) -> u8 {
        let mut tag = self.next_down_tag.lock().await;
        let current = *tag & 0x7F;
        *tag = (*tag + 1) & 0x7F;
        current
    }

    pub fn get_session_sender(&self, player_id: i64) -> Option<mpsc::Sender<CommandPacket>> {
        self.sessions.get(&player_id).map(|v| v.value().clone())
    }

    pub fn register_session(&self, player_id: i64, outbound: mpsc::Sender<CommandPacket>) {
        self.sessions.insert(player_id, outbound);
    }

    pub fn unregister_session(&self, player_id: i64) {
        self.sessions.remove(&player_id);
    }

    pub async fn disconnect_session(&self, player_id: i64) -> bool {
        let Some((_, sender)) = self.sessions.remove(&player_id) else {
            return false;
        };
        let _ = sender.send(CommandPacket::Disconnect).await;
        true
    }

    pub fn online_player_ids(&self) -> Vec<i64> {
        let mut players = self
            .sessions
            .iter()
            .map(|entry| *entry.key())
            .collect::<Vec<_>>();
        players.sort();
        players
    }
}

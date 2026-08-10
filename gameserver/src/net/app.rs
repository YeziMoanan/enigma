use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

use crate::net::outbound::CommandPacket;

/// App-level shared state
pub struct AppState {
    pub db: &'static SqlitePool,
    pub tables: &'static config::GameDB,
    sessions: dashmap::DashMap<i64, Arc<SessionHandle>>,
    session_locks: dashmap::DashMap<i64, Arc<Mutex<()>>>,
}

/// The sender and identity of one TCP connection.
///
/// Keeping the handle in the session registry lets disconnect cleanup verify
/// that it is removing the same connection that was registered, rather than
/// accidentally removing a newer login for the same player.
pub struct SessionHandle {
    pub sender: mpsc::Sender<CommandPacket>,
}

#[allow(dead_code)]
impl AppState {
    pub fn new(db: SqlitePool, tables: &'static config::GameDB) -> Self {
        Self {
            db: Box::leak(Box::new(db)),
            tables,
            sessions: dashmap::DashMap::new(),
            session_locks: dashmap::DashMap::new(),
        }
    }

    pub fn get_session_sender(&self, player_id: i64) -> Option<mpsc::Sender<CommandPacket>> {
        self.sessions
            .get(&player_id)
            .map(|v| v.value().sender.clone())
    }

    pub fn get_session_handle(&self, player_id: i64) -> Option<Arc<SessionHandle>> {
        self.sessions.get(&player_id).map(|v| v.value().clone())
    }

    pub fn register_session(&self, player_id: i64, session: Arc<SessionHandle>) {
        self.sessions.insert(player_id, session);
    }

    pub async fn lock_session(&self, player_id: i64) -> tokio::sync::OwnedMutexGuard<()> {
        self.session_locks
            .entry(player_id)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
            .lock_owned()
            .await
    }

    pub fn is_current_session(&self, player_id: i64, session: &Arc<SessionHandle>) -> bool {
        self.sessions
            .get(&player_id)
            .is_some_and(|current| Arc::ptr_eq(current.value(), session))
    }

    pub fn unregister_session(&self, player_id: i64, session: &Arc<SessionHandle>) -> bool {
        let Some(entry) = self.sessions.get(&player_id) else {
            return false;
        };
        if !Arc::ptr_eq(entry.value(), session) {
            return false;
        }
        drop(entry);
        self.sessions
            .remove_if(&player_id, |_, current| Arc::ptr_eq(current, session))
            .is_some()
    }

    pub fn unregister_session_if_current(
        &self,
        player_id: i64,
        session: &Arc<SessionHandle>,
    ) -> bool {
        self.unregister_session(player_id, session)
    }

    pub async fn disconnect_session(&self, player_id: i64) -> bool {
        let Some((_, session)) = self.sessions.remove(&player_id) else {
            return false;
        };
        let _ = session.sender.send(CommandPacket::Disconnect).await;
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

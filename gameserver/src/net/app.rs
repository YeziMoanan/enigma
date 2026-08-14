use sqlx::SqlitePool;
use std::{sync::Arc, time::{Duration, Instant}};
use tokio::sync::{Mutex, mpsc};

use crate::net::outbound::CommandPacket;

/// App-level shared state
pub struct AppState {
    pub db: &'static SqlitePool,
    pub tables: &'static config::GameDB,
    sessions: dashmap::DashMap<i64, Arc<SessionHandle>>,
    session_locks: dashmap::DashMap<i64, Arc<Mutex<()>>>,
    rate_limits: dashmap::DashMap<i64, Arc<Mutex<RateLimitState>>>,
}

#[derive(Default)]
struct RateLimitState {
    last_purchase: Option<Instant>,
    last_summon: Option<Instant>,
}

#[derive(Clone, Copy)]
enum RateLimitKind {
    Purchase,
    Summon,
}

/// The sender and identity of one TCP connection.
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
            rate_limits: dashmap::DashMap::new(),
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

    pub async fn allow_purchase(&self, player_id: i64) -> bool {
        self.allow_rate_limited(player_id, Duration::from_secs(2), RateLimitKind::Purchase)
            .await
    }

    pub async fn allow_summon(&self, player_id: i64) -> bool {
        self.allow_rate_limited(player_id, Duration::from_secs(5), RateLimitKind::Summon)
            .await
    }

    async fn allow_rate_limited(
        &self,
        player_id: i64,
        interval: Duration,
        kind: RateLimitKind,
    ) -> bool {
        let lock = self
            .rate_limits
            .entry(player_id)
            .or_insert_with(|| Arc::new(Mutex::new(RateLimitState::default())))
            .clone();
        let mut state = lock.lock().await;
        allow_slot(&mut state, Instant::now(), interval, kind)
    }

    pub fn is_current_session(
        &self,
        player_id: i64,
        outbound: &mpsc::Sender<CommandPacket>,
    ) -> bool {
        self.sessions
            .get(&player_id)
            .is_some_and(|current| current.sender.same_channel(outbound))
    }

    pub fn unregister_session(&self, player_id: i64, session: &Arc<SessionHandle>) -> bool {
        self.sessions
            .remove_if(&player_id, |_, current| Arc::ptr_eq(current, session))
            .is_some()
    }

    pub async fn disconnect_session(&self, player_id: i64) -> bool {
        let Some((_, session)) = self.sessions.remove(&player_id) else {
            return false;
        };
        let _ = session.sender.send(CommandPacket::Disconnect).await;
        true
    }

    pub fn unregister_session_if_current(
        &self,
        player_id: i64,
        outbound: &mpsc::Sender<CommandPacket>,
    ) -> bool {
        match self.sessions.entry(player_id) {
            dashmap::mapref::entry::Entry::Occupied(entry)
                if entry.get().sender.same_channel(outbound) =>
            {
                entry.remove();
                true
            }
            _ => false,
        }
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

fn allow_slot(
    state: &mut RateLimitState,
    now: Instant,
    interval: Duration,
    kind: RateLimitKind,
) -> bool {
    let slot = match kind {
        RateLimitKind::Purchase => &mut state.last_purchase,
        RateLimitKind::Summon => &mut state.last_summon,
    };
    if slot
        .as_ref()
        .is_some_and(|last| now.duration_since(*last) < interval)
    {
        return false;
    }
    *slot = Some(now);
    true
}

#[cfg(test)]
mod rate_limit_tests {
    use super::*;

    #[test]
    fn purchase_and_summon_limits_are_independent() {
        let mut state = RateLimitState::default();
        let now = Instant::now();
        assert!(allow_slot(
            &mut state,
            now,
            Duration::from_secs(2),
            RateLimitKind::Purchase
        ));
        assert!(!allow_slot(
            &mut state,
            now + Duration::from_millis(1999),
            Duration::from_secs(2),
            RateLimitKind::Purchase
        ));
        assert!(allow_slot(
            &mut state,
            now + Duration::from_secs(2),
            Duration::from_secs(2),
            RateLimitKind::Purchase
        ));
        assert!(allow_slot(
            &mut state,
            now,
            Duration::from_secs(5),
            RateLimitKind::Summon
        ));
        assert!(!allow_slot(
            &mut state,
            now + Duration::from_millis(4999),
            Duration::from_secs(5),
            RateLimitKind::Summon
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn stale_session_cannot_unregister_replacement() {
        let data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data/excel2json");
        let _ = config::init(data_dir.to_str().unwrap());
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let state = Arc::new(AppState::new(pool, config::configs::get()));
        let (first, _) = mpsc::channel(1);
        let (replacement, _) = mpsc::channel(1);
        let first_session = Arc::new(SessionHandle {
            sender: first.clone(),
        });
        let replacement_session = Arc::new(SessionHandle {
            sender: replacement.clone(),
        });

        state.register_session(7, first_session);
        let teardown = state.lock_session(7).await;
        let replacement_state = Arc::clone(&state);
        let replacement_sender = replacement.clone();
        let replacement_task = tokio::spawn(async move {
            let _registration = replacement_state.lock_session(7).await;
            replacement_state.register_session(
                7,
                Arc::new(SessionHandle {
                    sender: replacement_sender,
                }),
            );
        });

        tokio::task::yield_now().await;
        assert!(state.is_current_session(7, &first));
        assert!(!replacement_task.is_finished());
        assert!(state.unregister_session_if_current(7, &first));
        drop(teardown);
        replacement_task.await.unwrap();

        assert!(!state.unregister_session_if_current(7, &first));
        assert!(
            state
                .get_session_sender(7)
                .unwrap()
                .same_channel(&replacement)
        );
        assert!(state.unregister_session_if_current(7, &replacement));
        assert!(state.get_session_sender(7).is_none());
    }
}

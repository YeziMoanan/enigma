use common::time::ServerTime;
use database::db::player_state;
use prost::Message;
use std::sync::Arc;

use tokio::sync::mpsc;

use sonettobuf::CmdId;

use crate::{
    error::AppError,
    net::{
        app::{AppState, SessionHandle},
        outbound::{CommandPacket, DownTag},
    },
    player::{Player, PlayerState},
    util::{common::encode_message, push},
};

#[allow(dead_code)]
pub struct ConnectionContext {
    pub state: &'static AppState,
    pub outbound: mpsc::Sender<CommandPacket>,
    pub session: Arc<SessionHandle>,

    pub player: Option<Player>,

    pub logged_in: bool,
    disconnect_requested: bool,

    next_sequence: u32,
}

#[allow(dead_code)]
impl ConnectionContext {
    pub fn new(outbound: mpsc::Sender<CommandPacket>, state: &'static AppState) -> Self {
        let session = Arc::new(SessionHandle {
            sender: outbound.clone(),
        });
        Self {
            state,
            outbound,
            session,
            player: None,
            logged_in: false,
            disconnect_requested: false,
            next_sequence: 0,
        }
    }

    pub async fn load_player(&mut self, player_id: i64) -> Result<(), AppError> {
        self.logged_in = true;
        let now = ServerTime::now_ms();

        let mut state: PlayerState = player_state::load(self.state.db, player_id)
            .await?
            .ok_or_else(|| AppError::Custom(format!("Missing player state for {player_id}")))?
            .into();

        state.last_login_timestamp = Some(now);
        state.updated_at = now;

        self.player = Some(Player::new(player_id, state));
        tracing::info!("Loaded player state for player {player_id}");
        Ok(())
    }

    async fn save_player_state(&self, state: &PlayerState) -> Result<(), AppError> {
        player_state::save(self.state.db, &state.into()).await?;
        Ok(())
    }

    pub async fn save_player(&self) -> Result<(), AppError> {
        if let Some(player) = &self.player {
            self.save_player_state(&player.state).await?;
        }
        Ok(())
    }

    pub fn player(&self) -> Result<&Player, AppError> {
        self.player.as_ref().ok_or(AppError::NotLoggedIn)
    }

    pub fn player_mut(&mut self) -> Result<&mut Player, AppError> {
        self.player.as_mut().ok_or(AppError::NotLoggedIn)
    }

    pub fn next_sequence(&mut self) -> u32 {
        let seq = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        seq
    }

    pub fn request_disconnect(&mut self) {
        self.disconnect_requested = true;
    }

    pub fn should_disconnect(&self) -> bool {
        self.disconnect_requested
    }

    pub async fn notify<T: Message>(&mut self, cmd_id: CmdId, msg: T) -> Result<(), AppError> {
        let body = encode_message(&msg)?;

        let packet = CommandPacket::Push {
            cmd_id,
            body,
            down_tag: DownTag::Next,
        };

        self.send_packet(packet).await?;
        Ok(())
    }

    pub async fn push_red_dot(
        &mut self,
        define_id: i32,
        info_ids: Vec<i32>,
        replace_all: bool,
    ) -> Result<(), AppError> {
        push::send_red_dot_push(self, define_id, info_ids, replace_all).await
    }

    pub async fn push_red_dot_value(
        &mut self,
        define_id: i32,
        info_ids: Vec<i32>,
        replace_all: bool,
        value: i32,
        time: i32,
    ) -> Result<(), AppError> {
        push::send_red_dot_value_push(self, define_id, info_ids, replace_all, value, time).await
    }

    pub async fn send_reply<T: Message>(
        &mut self,
        cmd_id: CmdId,
        msg: T,
        result_code: i16,
        up_tag: u8,
    ) -> Result<(), AppError> {
        let body = encode_message(&msg)?;

        let packet = CommandPacket::Reply {
            cmd_id,
            body,
            result_code,
            up_tag,
            down_tag: DownTag::Next,
        };

        self.send_packet(packet).await?;
        Ok(())
    }

    pub async fn send_raw_reply_fixed(
        &mut self,
        cmd_id: CmdId,
        body: Vec<u8>,
        result_code: i16,
        up_tag: u8,
    ) -> Result<(), AppError> {
        let packet = CommandPacket::Reply {
            cmd_id,
            body,
            result_code,
            up_tag,
            down_tag: DownTag::Fixed(255),
        };

        self.send_packet(packet).await?;
        Ok(())
    }

    // these messages are sent with fixed down_tag
    pub async fn send_reply_fixed<T: Message>(
        &mut self,
        cmd_id: CmdId,
        msg: T,
        result_code: i16,
        up_tag: u8,
    ) -> Result<(), AppError> {
        let body = encode_message(&msg)?;
        let packet = CommandPacket::Reply {
            cmd_id,
            body,
            result_code,
            up_tag,
            down_tag: DownTag::Fixed(255),
        };

        self.send_packet(packet).await?;
        Ok(())
    }

    pub async fn send_empty_reply(
        &mut self,
        cmd_id: CmdId,
        body: Vec<u8>,
        result_code: i16,
        up_tag: u8,
    ) -> Result<(), AppError> {
        let packet = CommandPacket::Reply {
            cmd_id,
            body,
            result_code,
            up_tag,
            down_tag: DownTag::Next,
        };

        self.send_packet(packet).await?;
        Ok(())
    }

    async fn send_packet(&mut self, packet: CommandPacket) -> Result<(), AppError> {
        self.outbound
            .send(packet)
            .await
            .map_err(|e| AppError::Custom(format!("failed to queue outbound packet: {e}")))
    }

    pub fn register(&self) {
        if let Ok(player) = self.player() {
            self.state.register_session(player.id, self.session.clone());
            tracing::info!("Registered session for player {}", player.id);
        } else {
            tracing::warn!("Attempted to register session without player_id");
        }
    }
}

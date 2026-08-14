use sonettobuf::{CmdId, prost};
use thiserror::Error;
use tokio::io;

#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum AppError {
    #[error("Tokio IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Packet error: {0}")]
    Packet(#[from] PacketError),

    #[error("Command error: {0}")]
    Cmd(#[from] CmdError),

    #[error("Serde JSON error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Request decode failed: {0}")]
    Decode(#[from] prost::DecodeError),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Game logic error: {0}")]
    Logic(#[from] logic::LogicError),

    #[error("User is not logged in")]
    NotLoggedIn,

    #[error("Custom error: {0}")]
    Custom(String),

    #[error("Missing Player id")]
    MissingPlayerId,

    #[error("Invalid request")]
    InvalidRequest,

    #[error("Request rate limited")]
    RateLimited,

    #[error("Invalid battle checkpoint: {0}")]
    InvalidBattleCheckpoint(String),

    #[error("Hero not found")]
    HeroNotFound,

    #[error("Insufficient items")]
    InsufficientItems,

    #[error("Insufficient funds")]
    InsufficientCurrency,

    #[error("Banner not found")]
    BannerNotFound,

    #[error("Banner is not yet active")]
    BannerNotYetActive,

    #[error("Banner has expired")]
    BannerExpired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientErrorAction {
    Reply(ClientResultCode),
    Reconnect(ClientResultCode),
}

#[repr(i16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientResultCode {
    Success = 0,
    ServerError = -1,
    ServiceUnavailable = -3,
    ParameterError = -4,
    InvalidOperation = -21,
    InsufficientResources = -22,
    InsufficientItems = -24,
}

impl ClientResultCode {
    pub const fn id(self) -> i16 {
        self as i16
    }
}

impl AppError {
    pub const fn client_action(&self) -> ClientErrorAction {
        use ClientErrorAction::{Reconnect, Reply};
        use ClientResultCode::{
            InsufficientItems, InsufficientResources, InvalidOperation, ParameterError,
            ServerError, ServiceUnavailable, Success,
        };

        match self {
            Self::Cmd(CmdError::UnhandledCmd(_)) => Reply(Success),
            Self::Decode(_) | Self::InvalidRequest => Reply(ParameterError),
            Self::HeroNotFound
            | Self::BannerNotFound
            | Self::BannerNotYetActive
            | Self::BannerExpired => Reply(InvalidOperation),
            Self::InsufficientItems => Reply(InsufficientItems),
            Self::InsufficientCurrency => Reply(InsufficientResources),
            Self::Logic(logic::LogicError::InvalidRequest) => Reply(ParameterError),
            Self::Logic(
                logic::LogicError::HeroNotFound
                | logic::LogicError::BannerNotFound
                | logic::LogicError::BannerNotYetActive
                | logic::LogicError::BannerExpired,
            ) => Reply(InvalidOperation),
            Self::Logic(logic::LogicError::InsufficientItems) => Reply(InsufficientItems),
            Self::Logic(logic::LogicError::InsufficientCurrency) => Reply(InsufficientResources),
            Self::RateLimited => Reply(ServiceUnavailable),
            Self::Cmd(_) | Self::NotLoggedIn | Self::MissingPlayerId => {
                Reconnect(ServiceUnavailable)
            }
            Self::Io(_)
            | Self::Packet(_)
            | Self::Serde(_)
            | Self::Database(_)
            | Self::Logic(_)
            | Self::Custom(_)
            | Self::InvalidBattleCheckpoint(_) => Reconnect(ServerError),
        }
    }
}

impl From<std::str::Utf8Error> for AppError {
    fn from(e: std::str::Utf8Error) -> Self {
        AppError::Packet(PacketError::Custom(format!("UTF-8 error: {}", e)))
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        AppError::Custom(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_errors_and_unsupported_commands_reply_without_reconnecting() {
        assert_eq!(
            AppError::InvalidRequest.client_action(),
            ClientErrorAction::Reply(ClientResultCode::ParameterError)
        );
        assert_eq!(
            AppError::Cmd(CmdError::UnhandledCmd(CmdId::GetServerTimeCmd)).client_action(),
            ClientErrorAction::Reply(ClientResultCode::Success)
        );
    }
}

#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum PacketError {
    #[error("Packet length less than header (expected: {0}, actual: {1})")]
    LengthLessThanHeader(usize, usize),

    #[error("Packet length mismatch (expected: {0}, actual: {1})")]
    LengthMismatch(usize, usize),

    #[error("Client packet data decode failed: {0}")]
    ClientPacketDataDecodeFail(#[from] prost::DecodeError),

    #[error("Server packet data decode failed: {0}")]
    ServerPacketDataDecodeFail(prost::DecodeError),

    #[error("Packet error: {0}")]
    Custom(String),
}

#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum CmdError {
    #[error("Unregistered Cmd: {0}")]
    UnregisteredCmd(i16),

    #[error("Unhandled Cmd: {0:?}")]
    UnhandledCmd(CmdId),

    #[error("Received server packet as client request")]
    ServerPacketReceivedAsClient,
}

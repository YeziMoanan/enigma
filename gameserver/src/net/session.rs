use crate::net::context::ConnectionContext;
use crate::net::{
    app::AppState,
    outbound::{CommandPacket, DownTag},
    router,
};
use crate::util::common::send_raw_server_message;
use byteorder::{BE, ByteOrder};
use tokio::{
    io::{AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
};

const MAX_CLIENT_PACKET_LEN: usize = 8 * 1024 * 1024;

#[allow(dead_code)]
pub async fn handle_client(socket: TcpStream, state: &'static AppState) -> anyhow::Result<()> {
    let (mut reader, writer) = socket.into_split();
    let (outbound_tx, outbound_rx) = mpsc::channel(64);
    let write_task = tokio::spawn(write_loop(writer, outbound_rx));
    let mut ctx = ConnectionContext::new(outbound_tx, state);

    let result = loop {
        let packet = {
            let mut header = [0u8; 4];
            if let Err(e) = reader.read_exact(&mut header).await {
                tracing::debug!("Client disconnected: {e}");
                break Ok(());
            }

            let raw_packet_len = BE::read_i32(&header);
            let Some(packet_len) = validated_client_packet_len(raw_packet_len) else {
                tracing::warn!(
                    "Rejected invalid client packet length: {} (maximum {} bytes)",
                    raw_packet_len,
                    MAX_CLIENT_PACKET_LEN
                );
                break Ok(());
            };
            let mut buffer = vec![0u8; packet_len];
            if let Err(e) = reader.read_exact(&mut buffer).await {
                tracing::warn!("Failed to read packet body ({} bytes): {e}", packet_len);
                break Ok(());
            }

            let mut packet = Vec::with_capacity(4 + packet_len);
            packet.extend_from_slice(&header);
            packet.extend_from_slice(&buffer);
            packet
        };

        if let Err(e) = router::dispatch_command(&mut ctx, packet).await {
            tracing::error!("Dispatch error: {e}");
            break Err(e.into());
        }
        if ctx.should_disconnect() {
            break Ok(());
        }
    };

    let player_id = if let Ok(player) = ctx.player() {
        let player_id = player.id;
        let _session = ctx.state.lock_session(player_id).await;
        if ctx.state.is_current_session(player_id, &ctx.session) {
            if let Err(e) =
                database::db::game::power_maker::record_logout(ctx.state.db, player_id).await
            {
                tracing::error!(
                    "Failed to record power maker logout for {}: {}",
                    player_id,
                    e
                );
            }
            if let Err(e) = ctx.save_player().await {
                tracing::error!("Failed to save player state for {}: {}", player_id, e);
            }
            ctx.state
                .unregister_session_if_current(player_id, &ctx.session);
            Some(player_id)
        } else {
            None
        }
    } else {
        None
    };

    if let Some(player_id) = player_id {
        tracing::warn!("Player {} disconnected and saved progress", player_id);
    }

    drop(ctx);
    write_task.await??;
    result
}

fn validated_client_packet_len(raw_packet_len: i32) -> Option<usize> {
    let packet_len = usize::try_from(raw_packet_len).ok()?;
    (1..=MAX_CLIENT_PACKET_LEN)
        .contains(&packet_len)
        .then_some(packet_len)
}

async fn write_loop<W>(mut writer: W, mut rx: mpsc::Receiver<CommandPacket>) -> anyhow::Result<()>
where
    W: AsyncWrite + Unpin + Send,
{
    // Sequence numbers belong to one TCP connection and are assigned only
    // after the outbound queue has serialized packet order.
    let mut next_down_tag = 0u8;
    while let Some(packet) = rx.recv().await {
        match packet {
            CommandPacket::Disconnect => break,
            CommandPacket::Push {
                cmd_id,
                body,
                down_tag,
            } => {
                let down_tag = resolve_down_tag(down_tag, &mut next_down_tag);
                send_raw_server_message(&mut writer, cmd_id, body, 0, 255, down_tag).await?;
            }
            CommandPacket::Reply {
                cmd_id,
                body,
                result_code,
                up_tag,
                down_tag,
            } => {
                let down_tag = resolve_down_tag(down_tag, &mut next_down_tag);
                send_raw_server_message(&mut writer, cmd_id, body, result_code, up_tag, down_tag)
                    .await?;
            }
        }
    }

    writer.shutdown().await?;
    Ok(())
}

fn resolve_down_tag(tag: DownTag, next: &mut u8) -> u8 {
    match tag {
        DownTag::Fixed(value) => value,
        DownTag::Next => {
            let current = *next & 0x7F;
            *next = (*next + 1) & 0x7F;
            current
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_CLIENT_PACKET_LEN, resolve_down_tag, validated_client_packet_len, write_loop};
    use crate::net::outbound::{CommandPacket, DownTag};
    use byteorder::{BE, ByteOrder};
    use sonettobuf::CmdId;
    use tokio::io::{AsyncReadExt, duplex, split};
    use tokio::sync::mpsc;

    #[test]
    fn client_packet_length_is_bounded_before_allocation() {
        assert_eq!(validated_client_packet_len(1), Some(1));
        assert_eq!(
            validated_client_packet_len(MAX_CLIENT_PACKET_LEN as i32),
            Some(MAX_CLIENT_PACKET_LEN)
        );
        assert_eq!(validated_client_packet_len(0), None);
        assert_eq!(validated_client_packet_len(-1), None);
        assert_eq!(
            validated_client_packet_len(MAX_CLIENT_PACKET_LEN as i32 + 1),
            None
        );
        assert_eq!(validated_client_packet_len(369_295_617), None);
    }

    #[test]
    fn down_tags_are_connection_local_and_wrap_at_protocol_limit() {
        let mut first = 0;
        let mut second = 0;
        assert_eq!(resolve_down_tag(DownTag::Next, &mut first), 0);
        assert_eq!(resolve_down_tag(DownTag::Next, &mut second), 0);
        assert_eq!(resolve_down_tag(DownTag::Next, &mut first), 1);
        assert_eq!(resolve_down_tag(DownTag::Fixed(255), &mut first), 255);

        first = 127;
        assert_eq!(resolve_down_tag(DownTag::Next, &mut first), 127);
        assert_eq!(resolve_down_tag(DownTag::Next, &mut first), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn one_hundred_connection_writers_keep_independent_sequences() {
        let mut tasks = Vec::with_capacity(100);

        for _ in 0..100 {
            tasks.push(tokio::spawn(async move {
                let (server, mut client) = duplex(4096);
                let (_, writer) = split(server);
                let (tx, rx) = mpsc::channel(16);
                let writer_task = tokio::spawn(write_loop(writer, rx));

                for _ in 0..4 {
                    tx.send(CommandPacket::Push {
                        cmd_id: CmdId::GetServerTimeCmd,
                        body: vec![0],
                        down_tag: DownTag::Next,
                    })
                    .await
                    .unwrap();
                }
                drop(tx);

                for expected in 0..4u8 {
                    let mut length = [0u8; 4];
                    client.read_exact(&mut length).await.unwrap();
                    let body_len = BE::read_u32(&length) as usize;
                    let mut body = vec![0u8; body_len];
                    client.read_exact(&mut body).await.unwrap();
                    assert_eq!(body[5], expected, "down_tag sequence escaped connection");
                }

                writer_task.await.unwrap().unwrap();
            }));
        }

        for task in tasks {
            task.await.unwrap();
        }
    }
}

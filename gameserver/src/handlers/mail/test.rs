use super::on_read_mail;
use crate::{
    net::{
        app::AppState, context::ConnectionContext, outbound::CommandPacket, packet::ClientPacket,
    },
    player::{Player, PlayerState},
};
use config::configs;
use prost::Message;
use sonettobuf::{CmdId, CurrencyChangePush, ReadMailReply, ReadMailRequest};
use sqlx::SqlitePool;
use tokio::sync::mpsc;

#[tokio::test]
async fn mail_claim_replies_before_pushing_raindrop_snapshots() {
    let data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data/excel2json");
    let _ = config::init(data_dir.to_str().unwrap());

    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    database::run_migrations(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO users (id, username, created_at, updated_at)
         VALUES (42, 'mail-currency-push', 0, 0);
         INSERT INTO user_mails
             (incr_id, user_id, mail_id, attachment, create_time, expire_time)
         VALUES (420, 42, 0, '2#1#2|2#2#3', 0, 0);",
    )
    .execute(&pool)
    .await
    .unwrap();

    let state = Box::leak(Box::new(AppState::new(pool, configs::get())));
    let (outbound, mut packets) = mpsc::channel(8);
    let mut ctx = ConnectionContext::new(outbound, state);
    ctx.player = Some(Player::new(42, PlayerState::new(42, 0)));

    let mut data = Vec::new();
    ReadMailRequest { incr_id: Some(420) }
        .encode(&mut data)
        .unwrap();
    on_read_mail(
        &mut ctx,
        ClientPacket {
            sequence: 0,
            cmd_id: CmdId::ReadMailCmd as i16,
            up_tag: 7,
            data,
        },
    )
    .await
    .unwrap();

    let CommandPacket::Reply {
        cmd_id: CmdId::ReadMailCmd,
        body,
        up_tag: 7,
        ..
    } = packets.recv().await.unwrap()
    else {
        panic!("mail claim reply must precede reward pushes");
    };
    assert_eq!(ReadMailReply::decode(&*body).unwrap().incr_id, Some(420));

    let CommandPacket::Push {
        cmd_id: CmdId::CurrencyChangePushCmd,
        body,
        ..
    } = packets.recv().await.unwrap()
    else {
        panic!("mail claim must push authoritative currency snapshots");
    };
    let push = CurrencyChangePush::decode(&*body).unwrap();
    let mut balances = push
        .change_currency
        .into_iter()
        .filter_map(|currency| Some((currency.currency_id?, currency.quantity?)))
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(balances.remove(&1), Some(2));
    assert_eq!(balances.remove(&2), Some(3));

    let rows: Vec<(i32, i64)> = sqlx::query_as(
        "SELECT currency_id, quantity FROM currencies
         WHERE user_id = 42 AND currency_id IN (1, 2)
         ORDER BY currency_id",
    )
    .fetch_all(state.db)
    .await
    .unwrap();
    assert_eq!(rows, vec![(1, 2), (2, 3)]);
}

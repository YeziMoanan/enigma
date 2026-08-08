use crate::db::game::buildings;
use crate::models::game::{
    block_packages::{BlockInfo, BlockPackage, RoadInfo, SpecialBlock},
    room_ob::RoomProductionLine,
};
use anyhow::{Result, bail};
use common::time::ServerTime;
use serde::Deserialize;
use sonettobuf::GetRoomInfoReply;
use sqlx::{Sqlite, SqliteConnection, SqlitePool, Transaction};
use std::{
    collections::{BTreeSet, HashMap},
    sync::OnceLock,
};

const INITIAL_BLOCKS_JSON: &str = r#"[
    {"blockId":-1,"x":0,"y":0,"rotate":0,"waterType":-1,"blockColor":-1,"buildDegree":8},
    {"blockId":-2,"x":1,"y":-1,"rotate":0,"waterType":-1,"blockColor":-1,"buildDegree":8},
    {"blockId":-3,"x":0,"y":-1,"rotate":0,"waterType":-1,"blockColor":-1,"buildDegree":8},
    {"blockId":-4,"x":1,"y":0,"rotate":0,"waterType":-1,"blockColor":-1,"buildDegree":8},
    {"blockId":-5,"x":-1,"y":0,"rotate":0,"waterType":-1,"blockColor":-1,"buildDegree":8},
    {"blockId":-6,"x":1,"y":-2,"rotate":0,"waterType":-1,"blockColor":-1,"buildDegree":8}
]"#;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitialBlock {
    block_id: i32,
    x: i32,
    y: i32,
    rotate: i32,
    water_type: i32,
    block_color: i32,
    build_degree: i32,
}

fn initial_blocks() -> &'static [InitialBlock] {
    static BLOCKS: OnceLock<Vec<InitialBlock>> = OnceLock::new();
    BLOCKS.get_or_init(|| {
        serde_json::from_str(INITIAL_BLOCKS_JSON).expect("embedded initial room blocks are valid")
    })
}

fn is_initial_block(block_id: i32) -> bool {
    initial_blocks()
        .iter()
        .any(|initial| initial.block_id == block_id)
}

pub fn initial_block_build_degree(block_id: i32) -> Option<i32> {
    initial_blocks()
        .iter()
        .find(|initial| initial.block_id == block_id)
        .map(|initial| initial.build_degree)
}

pub struct RoomState {
    pub room_level: i32,
    pub room_theme_ids: Vec<i32>,
    pub room_skin_ids: Vec<i32>,
    pub have_fishing_bonus: bool,
}

pub async fn seed_defaults(connection: &mut SqliteConnection, user_id: i64) -> sqlx::Result<()> {
    let tables = config::configs::get();
    for package in tables.block_package.iter().filter(|package| package.free) {
        sqlx::query(
            "INSERT OR IGNORE INTO user_block_packages
                (user_id, block_package_id, unused_block_ids, used_block_ids)
             VALUES (?, ?, '[]', '[]')",
        )
        .bind(user_id)
        .bind(package.id)
        .execute(&mut *connection)
        .await?;
    }
    for block in initial_blocks() {
        sqlx::query(
            "INSERT OR IGNORE INTO user_blocks
                (user_id, block_id, x, y, rotate, water_type, block_color)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(user_id)
        .bind(block.block_id)
        .bind(block.x)
        .bind(block.y)
        .bind(block.rotate)
        .bind(block.water_type)
        .bind(block.block_color)
        .execute(&mut *connection)
        .await?;
    }
    Ok(())
}

// Block Packages
pub async fn get_block_packages(pool: &SqlitePool, user_id: i64) -> Result<Vec<BlockPackage>> {
    Ok(sqlx::query_as::<_, BlockPackage>(
        "SELECT user_id, block_package_id, unused_block_ids, used_block_ids
         FROM user_block_packages
         WHERE user_id = ?
         ORDER BY rowid",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?)
}

pub async fn add_block_package(pool: &SqlitePool, user_id: i64, package_id: i32) -> Result<()> {
    sqlx::query(
        "INSERT INTO user_block_packages (user_id, block_package_id, unused_block_ids, used_block_ids)
         VALUES (?, ?, '[]', '[]')
         ON CONFLICT DO NOTHING"
    )
    .bind(user_id)
    .bind(package_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn add_block_package_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    package_id: i32,
) -> Result<BlockPackage> {
    sqlx::query(
        "INSERT INTO user_block_packages
             (user_id, block_package_id, unused_block_ids, used_block_ids)
         VALUES (?, ?, '[]', '[]')
         ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(package_id)
    .execute(&mut **tx)
    .await?;
    Ok(sqlx::query_as(
        "SELECT user_id, block_package_id, unused_block_ids, used_block_ids
         FROM user_block_packages
         WHERE user_id = ? AND block_package_id = ?",
    )
    .bind(user_id)
    .bind(package_id)
    .fetch_one(&mut **tx)
    .await?)
}

pub async fn update_block_package(
    pool: &SqlitePool,
    user_id: i64,
    package_id: i32,
    unused_block_ids: &[i32],
    used_block_ids: &[i32],
) -> Result<()> {
    let unused_json = serde_json::to_string(unused_block_ids)?;
    let used_json = serde_json::to_string(used_block_ids)?;

    sqlx::query(
        "UPDATE user_block_packages
         SET unused_block_ids = ?, used_block_ids = ?
         WHERE user_id = ? AND block_package_id = ?",
    )
    .bind(unused_json)
    .bind(used_json)
    .bind(user_id)
    .bind(package_id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn update_block_package_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    package_id: i32,
    unused_block_ids: &[i32],
    used_block_ids: &[i32],
) -> Result<()> {
    sqlx::query(
        "UPDATE user_block_packages
         SET unused_block_ids = ?, used_block_ids = ?
         WHERE user_id = ? AND block_package_id = ?",
    )
    .bind(serde_json::to_string(unused_block_ids)?)
    .bind(serde_json::to_string(used_block_ids)?)
    .bind(user_id)
    .bind(package_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

// Special Blocks
pub async fn get_special_blocks(pool: &SqlitePool, user_id: i64) -> Result<Vec<SpecialBlock>> {
    let blocks = sqlx::query_as::<_, SpecialBlock>(
        "SELECT user_id, block_id, create_time
         FROM user_special_blocks
         WHERE user_id = ?
         ORDER BY rowid",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(blocks)
}

pub async fn add_special_block(pool: &SqlitePool, user_id: i64, block_id: i32) -> Result<()> {
    let create_time = ServerTime::now_sec_i32();
    sqlx::query(
        "INSERT INTO user_special_blocks (user_id, block_id, create_time)
         VALUES (?, ?, ?)
         ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(block_id)
    .bind(create_time)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn add_special_block_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    block_id: i32,
) -> Result<SpecialBlock> {
    let create_time = ServerTime::now_sec_i32();
    sqlx::query(
        "INSERT INTO user_special_blocks (user_id, block_id, create_time)
         VALUES (?, ?, ?)
         ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(block_id)
    .bind(create_time)
    .execute(&mut **tx)
    .await?;
    Ok(sqlx::query_as(
        "SELECT user_id, block_id, create_time
         FROM user_special_blocks
         WHERE user_id = ? AND block_id = ?",
    )
    .bind(user_id)
    .bind(block_id)
    .fetch_one(&mut **tx)
    .await?)
}

// Placed Blocks
pub async fn get_blocks(pool: &SqlitePool, user_id: i64) -> Result<Vec<BlockInfo>> {
    let blocks = sqlx::query_as::<_, BlockInfo>(
        "SELECT user_id, block_id, x, y, rotate, water_type, block_color
         FROM user_blocks
         WHERE user_id = ?
         ORDER BY block_id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(blocks)
}

pub async fn save_block(pool: &SqlitePool, block: &BlockInfo) -> Result<()> {
    sqlx::query(
        "INSERT INTO user_blocks (user_id, block_id, x, y, rotate, water_type, block_color)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(user_id, block_id) DO UPDATE SET
             x = excluded.x,
             y = excluded.y,
             rotate = excluded.rotate,
             water_type = excluded.water_type,
             block_color = excluded.block_color",
    )
    .bind(block.user_id)
    .bind(block.block_id)
    .bind(block.x)
    .bind(block.y)
    .bind(block.rotate)
    .bind(block.water_type)
    .bind(block.block_color)
    .execute(pool)
    .await?;
    Ok(())
}

async fn save_block_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    block: &BlockInfo,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO user_blocks (user_id, block_id, x, y, rotate, water_type, block_color)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(user_id, block_id) DO UPDATE SET
             x = excluded.x,
             y = excluded.y,
             rotate = excluded.rotate,
             water_type = excluded.water_type,
             block_color = excluded.block_color",
    )
    .bind(block.user_id)
    .bind(block.block_id)
    .bind(block.x)
    .bind(block.y)
    .bind(block.rotate)
    .bind(block.water_type)
    .bind(block.block_color)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

// Roads
pub async fn get_roads(pool: &SqlitePool, user_id: i64) -> Result<Vec<RoadInfo>> {
    let roads = sqlx::query_as::<_, RoadInfo>(
        "SELECT user_id, id, from_type, to_type, road_points, critter_uid,
                building_uid, building_define_id, skin_id, block_clean_type
         FROM user_roads
         WHERE user_id = ?
         ORDER BY id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(roads)
}

pub async fn allot_road_critter(
    pool: &SqlitePool,
    user_id: i64,
    id: i32,
    critter_uid: i64,
) -> Result<()> {
    let road_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_roads WHERE user_id = ? AND id = ?)")
            .bind(user_id)
            .bind(id)
            .fetch_one(pool)
            .await?;
    if !road_exists {
        bail!("room road {id} does not exist");
    }
    if critter_uid != 0 {
        let owned: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM critters WHERE player_id = ? AND uid = ?)",
        )
        .bind(user_id)
        .bind(critter_uid)
        .fetch_one(pool)
        .await?;
        if !owned {
            bail!("critter {critter_uid} is not owned");
        }
        sqlx::query("UPDATE user_roads SET critter_uid = 0 WHERE user_id = ? AND critter_uid = ?")
            .bind(user_id)
            .bind(critter_uid)
            .execute(pool)
            .await?;
        sqlx::query("DELETE FROM critter_rest_info WHERE critter_uid = ?")
            .bind(critter_uid)
            .execute(pool)
            .await?;
    }
    sqlx::query("UPDATE user_roads SET critter_uid = ? WHERE user_id = ? AND id = ?")
        .bind(critter_uid)
        .bind(user_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn allot_road_vehicle(
    pool: &SqlitePool,
    user_id: i64,
    id: i32,
    building_uid: i64,
    skin_id: i32,
) -> Result<i32> {
    let define_id: Option<i32> =
        sqlx::query_scalar("SELECT define_id FROM user_buildings WHERE user_id = ? AND uid = ?")
            .bind(user_id)
            .bind(building_uid)
            .fetch_optional(pool)
            .await?;
    let define_id = define_id.ok_or_else(|| anyhow::anyhow!("room vehicle is not owned"))?;
    let changed = sqlx::query(
        "UPDATE user_roads
         SET building_uid = ?, building_define_id = ?, skin_id = ?
         WHERE user_id = ? AND id = ?",
    )
    .bind(building_uid)
    .bind(define_id)
    .bind(skin_id)
    .bind(user_id)
    .bind(id)
    .execute(pool)
    .await?;
    if changed.rows_affected() == 0 {
        bail!("room road {id} does not exist");
    }
    Ok(define_id)
}

// Room State
pub async fn get_room_reset_state(pool: &SqlitePool, user_id: i64) -> Result<bool> {
    let result: Option<(bool,)> =
        sqlx::query_as("SELECT is_reset FROM user_room_state WHERE user_id = ?")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;

    Ok(result.map(|(is_reset,)| is_reset).unwrap_or(false))
}

async fn begin_room_edit_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
) -> Result<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1 FROM user_room_state
             WHERE user_id = ? AND edit_snapshot IS NOT NULL
         )",
    )
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await?;
    if exists {
        return Ok(());
    }

    let snapshot = GetRoomInfoReply {
        infos: sqlx::query_as::<_, BlockInfo>(
            "SELECT user_id, block_id, x, y, rotate, water_type, block_color
             FROM user_blocks WHERE user_id = ? ORDER BY block_id",
        )
        .bind(user_id)
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .map(Into::into)
        .collect(),
        is_reset: Some(
            sqlx::query_scalar("SELECT is_reset FROM user_room_state WHERE user_id = ?")
                .bind(user_id)
                .fetch_optional(&mut **tx)
                .await?
                .unwrap_or(false),
        ),
        building_infos: buildings::get_placed_buildings_in_transaction(tx, user_id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
        block_packages: sqlx::query_as::<_, BlockPackage>(
            "SELECT user_id, block_package_id, unused_block_ids, used_block_ids
             FROM user_block_packages WHERE user_id = ? ORDER BY block_package_id",
        )
        .bind(user_id)
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .map(Into::into)
        .collect(),
        road_infos: sqlx::query_as::<_, RoadInfo>(
            "SELECT user_id, id, from_type, to_type, road_points, critter_uid,
                    building_uid, building_define_id, skin_id, block_clean_type
             FROM user_roads WHERE user_id = ? ORDER BY id",
        )
        .bind(user_id)
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .map(Into::into)
        .collect(),
    };
    sqlx::query(
        "INSERT INTO user_room_state (user_id, is_reset, last_reset_time, edit_snapshot)
         VALUES (?, 0, ?, ?)
         ON CONFLICT(user_id) DO UPDATE SET
             edit_snapshot = COALESCE(edit_snapshot, excluded.edit_snapshot)",
    )
    .bind(user_id)
    .bind(ServerTime::now_ms())
    .bind(serde_json::to_string(&snapshot)?)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn edit_roads(
    pool: &SqlitePool,
    user_id: i64,
    delete_ids: &[i32],
    roads: &[RoadInfo],
) -> Result<Vec<RoadInfo>> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    begin_room_edit_in_transaction(&mut tx, user_id).await?;
    for id in delete_ids {
        sqlx::query("DELETE FROM user_roads WHERE user_id = ? AND id = ?")
            .bind(user_id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    for road in roads {
        save_road_in_transaction(&mut tx, road).await?;
    }
    let current = sqlx::query_as::<_, RoadInfo>(
        "SELECT user_id, id, from_type, to_type, road_points, critter_uid,
                building_uid, building_define_id, skin_id, block_clean_type
         FROM user_roads WHERE user_id = ? ORDER BY id",
    )
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(current)
}

async fn save_road_in_transaction(tx: &mut Transaction<'_, Sqlite>, road: &RoadInfo) -> Result<()> {
    sqlx::query(
        "INSERT INTO user_roads (user_id, id, from_type, to_type, road_points,
                                  critter_uid, building_uid, building_define_id,
                                  skin_id, block_clean_type)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(user_id, id) DO UPDATE SET
             from_type = excluded.from_type,
             to_type = excluded.to_type,
             road_points = excluded.road_points,
             critter_uid = excluded.critter_uid,
             building_uid = excluded.building_uid,
             building_define_id = excluded.building_define_id,
             skin_id = excluded.skin_id,
             block_clean_type = excluded.block_clean_type",
    )
    .bind(road.user_id)
    .bind(road.id)
    .bind(road.from_type)
    .bind(road.to_type)
    .bind(&road.road_points)
    .bind(road.critter_uid)
    .bind(road.building_uid)
    .bind(road.building_define_id)
    .bind(road.skin_id)
    .bind(road.block_clean_type)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn place_building(
    pool: &SqlitePool,
    user_id: i64,
    uid: i64,
    placement: Option<(i32, i32, i32)>,
) -> Result<Option<crate::models::game::buildings::Building>> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    begin_room_edit_in_transaction(&mut tx, user_id).await?;
    let building = buildings::set_building_placement(&mut tx, user_id, uid, placement).await?;
    tx.commit().await?;
    Ok(building)
}

pub async fn committed_room_info(
    pool: &SqlitePool,
    user_id: i64,
) -> Result<Option<GetRoomInfoReply>> {
    let payload: Option<String> = sqlx::query_scalar(
        "SELECT edit_snapshot FROM user_room_state
         WHERE user_id = ? AND edit_snapshot IS NOT NULL",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    payload
        .map(|payload| serde_json::from_str(&payload).map_err(Into::into))
        .transpose()
}

pub async fn commit_room_edit(pool: &SqlitePool, user_id: i64) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO user_room_state (user_id, is_reset, last_reset_time, edit_snapshot)
         VALUES (?, 0, ?, NULL)
         ON CONFLICT(user_id) DO UPDATE SET
             is_reset = 0,
             last_reset_time = excluded.last_reset_time,
             edit_snapshot = NULL",
    )
    .bind(user_id)
    .bind(ServerTime::now_ms())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn revert_room_edit(pool: &SqlitePool, user_id: i64) -> Result<()> {
    let mut tx = pool.begin().await?;
    let payload: Option<String> = sqlx::query_scalar(
        "SELECT edit_snapshot FROM user_room_state
         WHERE user_id = ? AND edit_snapshot IS NOT NULL",
    )
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(snapshot) = payload
        .map(|payload| serde_json::from_str::<GetRoomInfoReply>(&payload))
        .transpose()?
    else {
        return Ok(());
    };
    let current_packages = sqlx::query_as::<_, BlockPackage>(
        "SELECT user_id, block_package_id, unused_block_ids, used_block_ids
         FROM user_block_packages
         WHERE user_id = ?
         ORDER BY rowid",
    )
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM user_blocks WHERE user_id = ?")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    for block in snapshot.infos {
        sqlx::query(
            "INSERT INTO user_blocks
             (user_id, block_id, x, y, rotate, water_type, block_color)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(user_id)
        .bind(block.block_id.unwrap_or_default())
        .bind(block.x.unwrap_or_default())
        .bind(block.y.unwrap_or_default())
        .bind(block.rotate.unwrap_or_default())
        .bind(block.water_type.unwrap_or_default())
        .bind(block.block_color.unwrap_or_default())
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query("UPDATE user_buildings SET in_use = 0, x = 0, y = 0, rotate = 0 WHERE user_id = ?")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    for building in snapshot.building_infos {
        sqlx::query(
            "UPDATE user_buildings
             SET in_use = ?, x = ?, y = ?, rotate = ?
             WHERE user_id = ? AND uid = ?",
        )
        .bind(building.r#use.unwrap_or_default())
        .bind(building.x.unwrap_or_default())
        .bind(building.y.unwrap_or_default())
        .bind(building.rotate.unwrap_or_default())
        .bind(user_id)
        .bind(building.uid.unwrap_or_default())
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query("DELETE FROM user_roads WHERE user_id = ?")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    for road in snapshot.road_infos {
        sqlx::query(
            "INSERT INTO user_roads
             (user_id, id, from_type, to_type, road_points, critter_uid,
              building_uid, building_define_id, skin_id, block_clean_type)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(user_id)
        .bind(road.id.unwrap_or_default())
        .bind(road.from_type.unwrap_or_default())
        .bind(road.to_type.unwrap_or_default())
        .bind(serde_json::to_string(&road.road_points)?)
        .bind(road.critter_uid.unwrap_or_default())
        .bind(road.building_uid.unwrap_or_default())
        .bind(road.building_define_id.unwrap_or_default())
        .bind(road.skin_id.unwrap_or_default())
        .bind(road.block_clean_type.unwrap_or_default())
        .execute(&mut *tx)
        .await?;
    }

    for package in current_packages {
        let mut unused: Vec<i32> = serde_json::from_str(&package.unused_block_ids)?;
        for block_id in serde_json::from_str::<Vec<i32>>(&package.used_block_ids)? {
            if !unused.contains(&block_id) {
                unused.push(block_id);
            }
        }
        sqlx::query(
            "UPDATE user_block_packages
             SET unused_block_ids = ?, used_block_ids = '[]'
             WHERE user_id = ? AND block_package_id = ?",
        )
        .bind(serde_json::to_string(&unused)?)
        .bind(user_id)
        .bind(package.block_package_id)
        .execute(&mut *tx)
        .await?;
    }
    for package in snapshot.block_packages {
        sqlx::query(
            "UPDATE user_block_packages
             SET unused_block_ids = ?, used_block_ids = ?
             WHERE user_id = ? AND block_package_id = ?",
        )
        .bind(serde_json::to_string(&package.un_use_block_ids)?)
        .bind(serde_json::to_string(&package.use_block_ids)?)
        .bind(user_id)
        .bind(package.block_package_id.unwrap_or_default())
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query(
        "INSERT INTO user_room_state (user_id, is_reset, last_reset_time)
         VALUES (?, ?, ?)
         ON CONFLICT(user_id) DO UPDATE SET is_reset = excluded.is_reset",
    )
    .bind(user_id)
    .bind(snapshot.is_reset.unwrap_or_default())
    .bind(ServerTime::now_ms())
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE user_room_state SET edit_snapshot = NULL WHERE user_id = ?")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn use_block(
    pool: &SqlitePool,
    user_id: i64,
    block_id: i32,
    package_id: i32,
    rotate: i32,
    x: i32,
    y: i32,
) -> Result<()> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let package: Option<BlockPackage> = sqlx::query_as(
        "SELECT user_id, block_package_id, unused_block_ids, used_block_ids
         FROM user_block_packages WHERE user_id = ? AND block_package_id = ?",
    )
    .bind(user_id)
    .bind(package_id)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(package) = package {
        if config::configs::get()
            .block_package
            .get(package_id)
            .is_none()
        {
            bail!("unknown room block package {package_id}");
        }
        let mut unused: Vec<i32> = serde_json::from_str(&package.unused_block_ids)?;
        let mut used: Vec<i32> = serde_json::from_str(&package.used_block_ids)?;
        unused.retain(|id| *id != block_id);
        if !used.contains(&block_id) {
            used.push(block_id);
        }
        begin_room_edit_in_transaction(&mut tx, user_id).await?;
        update_block_package_in_transaction(&mut tx, user_id, package_id, &unused, &used).await?;
    } else {
        let owned: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                 SELECT 1 FROM user_special_blocks WHERE user_id = ? AND block_id = ?
             )",
        )
        .bind(user_id)
        .bind(block_id)
        .fetch_one(&mut *tx)
        .await?;
        if !owned {
            bail!("room block {block_id} is not owned");
        }
        begin_room_edit_in_transaction(&mut tx, user_id).await?;
    }

    let existing: Option<(i32, i32)> = sqlx::query_as(
        "SELECT water_type, block_color FROM user_blocks
         WHERE user_id = ? AND block_id = ?",
    )
    .bind(user_id)
    .bind(block_id)
    .fetch_optional(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO user_blocks (user_id, block_id, x, y, rotate, water_type, block_color)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(user_id, block_id) DO UPDATE SET
             x = excluded.x, y = excluded.y, rotate = excluded.rotate,
             water_type = excluded.water_type, block_color = excluded.block_color",
    )
    .bind(user_id)
    .bind(block_id)
    .bind(x)
    .bind(y)
    .bind(rotate)
    .bind(existing.map_or(-1, |value| value.0))
    .bind(existing.map_or(-1, |value| value.1))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn unuse_blocks(pool: &SqlitePool, user_id: i64, block_ids: &[i32]) -> Result<()> {
    if block_ids.is_empty() {
        return Ok(());
    }
    if block_ids.iter().any(|block_id| is_initial_block(*block_id)) {
        bail!("initial room blocks cannot be removed");
    }
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    begin_room_edit_in_transaction(&mut tx, user_id).await?;
    for block_id in block_ids {
        sqlx::query("DELETE FROM user_blocks WHERE user_id = ? AND block_id = ?")
            .bind(user_id)
            .bind(block_id)
            .execute(&mut *tx)
            .await?;
    }
    let packages = sqlx::query_as::<_, BlockPackage>(
        "SELECT user_id, block_package_id, unused_block_ids, used_block_ids
         FROM user_block_packages WHERE user_id = ? ORDER BY block_package_id",
    )
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;
    for package in packages {
        let mut unused: Vec<i32> = serde_json::from_str(&package.unused_block_ids)?;
        let mut used: Vec<i32> = serde_json::from_str(&package.used_block_ids)?;
        let moved: Vec<_> = used
            .iter()
            .copied()
            .filter(|id| block_ids.contains(id))
            .collect();
        if moved.is_empty() {
            continue;
        }
        used.retain(|id| !block_ids.contains(id));
        for block_id in moved {
            if !unused.contains(&block_id) {
                unused.push(block_id);
            }
        }
        update_block_package_in_transaction(
            &mut tx,
            user_id,
            package.block_package_id,
            &unused,
            &used,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn replace_room_blocks(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    infos: &[sonettobuf::BlockInfo],
) -> Result<()> {
    let mut packages = sqlx::query_as::<_, BlockPackage>(
        "SELECT user_id, block_package_id, unused_block_ids, used_block_ids
         FROM user_block_packages WHERE user_id = ? ORDER BY block_package_id",
    )
    .bind(user_id)
    .fetch_all(&mut **tx)
    .await?;
    let special_blocks: BTreeSet<_> = sqlx::query_scalar::<_, i32>(
        "SELECT block_id FROM user_special_blocks WHERE user_id = ? ORDER BY rowid",
    )
    .bind(user_id)
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .collect();
    let mut block_packages = HashMap::new();

    for package in &packages {
        let unused: Vec<i32> = serde_json::from_str(&package.unused_block_ids)?;
        let used: Vec<i32> = serde_json::from_str(&package.used_block_ids)?;
        for block_id in unused.into_iter().chain(used) {
            block_packages.insert(block_id, package.block_package_id);
        }
    }

    let mut desired = BTreeSet::new();
    for info in infos {
        let block_id = info.block_id.unwrap_or_default();
        if !desired.insert(block_id) {
            bail!("room plan contains duplicate block {block_id}");
        }
        if block_id > 0
            && !block_packages.contains_key(&block_id)
            && !special_blocks.contains(&block_id)
        {
            bail!("room block {block_id} is not owned");
        }
    }

    for package in &mut packages {
        let unused: Vec<i32> = serde_json::from_str(&package.unused_block_ids)?;
        let used: Vec<i32> = serde_json::from_str(&package.used_block_ids)?;
        let mut known = Vec::with_capacity(unused.len() + used.len());
        for block_id in unused.into_iter().chain(used) {
            if !known.contains(&block_id) {
                known.push(block_id);
            }
        }
        let (used, unused): (Vec<_>, Vec<_>) = known
            .into_iter()
            .partition(|block_id| desired.contains(block_id));
        update_block_package_in_transaction(tx, user_id, package.block_package_id, &unused, &used)
            .await?;
    }

    sqlx::query("DELETE FROM user_blocks WHERE user_id = ?")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    for info in infos {
        sqlx::query(
            "INSERT INTO user_blocks
                 (user_id, block_id, x, y, rotate, water_type, block_color)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(user_id)
        .bind(info.block_id.unwrap_or_default())
        .bind(info.x.unwrap_or_default())
        .bind(info.y.unwrap_or_default())
        .bind(info.rotate.unwrap_or_default())
        .bind(info.water_type.unwrap_or(-1))
        .bind(info.block_color.unwrap_or(-1))
        .execute(&mut **tx)
        .await?;
    }

    sqlx::query("DELETE FROM user_roads WHERE user_id = ?")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE user_room_state SET edit_snapshot = NULL WHERE user_id = ?")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query(
        "INSERT INTO user_room_state (user_id, is_reset, last_reset_time)
         VALUES (?, ?, ?)
         ON CONFLICT(user_id) DO UPDATE SET is_reset = excluded.is_reset",
    )
    .bind(user_id)
    .bind(infos.is_empty())
    .bind(ServerTime::now_ms())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn set_water_types(
    pool: &SqlitePool,
    user_id: i64,
    changes: &[(i32, i32)],
) -> Result<Vec<BlockInfo>> {
    if changes.is_empty() {
        return Ok(Vec::new());
    }
    let mut tx = pool.begin().await?;
    let mut blocks = sqlx::query_as::<_, BlockInfo>(
        "SELECT user_id, block_id, x, y, rotate, water_type, block_color
         FROM user_blocks WHERE user_id = ? ORDER BY block_id",
    )
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;
    let mut changed = Vec::with_capacity(changes.len());

    for &(block_id, water_type) in changes {
        let block = blocks
            .iter_mut()
            .find(|block| block.block_id == block_id)
            .ok_or_else(|| anyhow::anyhow!("room block {block_id} is not placed"))?;
        block.water_type = water_type;
        changed.push(block.clone());
    }
    begin_room_edit_in_transaction(&mut tx, user_id).await?;
    for block in &changed {
        save_block_in_transaction(&mut tx, block).await?;
    }
    tx.commit().await?;
    Ok(changed)
}

pub async fn set_block_colors(
    pool: &SqlitePool,
    user_id: i64,
    changes: &[(i32, i32)],
) -> Result<Vec<BlockInfo>> {
    if changes.is_empty() {
        return Ok(Vec::new());
    }
    let mut tx = pool.begin().await?;
    let mut blocks = sqlx::query_as::<_, BlockInfo>(
        "SELECT user_id, block_id, x, y, rotate, water_type, block_color
         FROM user_blocks WHERE user_id = ? ORDER BY block_id",
    )
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;
    let mut changed = Vec::with_capacity(changes.len());

    for &(block_id, block_color) in changes {
        let block = blocks
            .iter_mut()
            .find(|block| block.block_id == block_id)
            .ok_or_else(|| anyhow::anyhow!("room block {block_id} is not placed"))?;
        block.block_color = block_color;
        changed.push(block.clone());
    }
    begin_room_edit_in_transaction(&mut tx, user_id).await?;
    for block in &changed {
        save_block_in_transaction(&mut tx, block).await?;
    }
    tx.commit().await?;
    Ok(changed)
}

pub async fn reset_room_edit(pool: &SqlitePool, user_id: i64) -> Result<()> {
    let mut tx = pool.begin().await?;
    begin_room_edit_in_transaction(&mut tx, user_id).await?;
    let block_ids: Vec<i32> =
        sqlx::query_scalar("SELECT block_id FROM user_blocks WHERE user_id = ?")
            .bind(user_id)
            .fetch_all(&mut *tx)
            .await?;
    for block_id in block_ids
        .into_iter()
        .filter(|block_id| !is_initial_block(*block_id))
    {
        sqlx::query("DELETE FROM user_blocks WHERE user_id = ? AND block_id = ?")
            .bind(user_id)
            .bind(block_id)
            .execute(&mut *tx)
            .await?;
    }
    let packages = sqlx::query_as::<_, BlockPackage>(
        "SELECT user_id, block_package_id, unused_block_ids, used_block_ids
         FROM user_block_packages WHERE user_id = ? ORDER BY block_package_id",
    )
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;
    for package in packages {
        let mut unused: Vec<i32> = serde_json::from_str(&package.unused_block_ids)?;
        for block_id in serde_json::from_str::<Vec<i32>>(&package.used_block_ids)? {
            if !unused.contains(&block_id) {
                unused.push(block_id);
            }
        }
        update_block_package_in_transaction(
            &mut tx,
            user_id,
            package.block_package_id,
            &unused,
            &[],
        )
        .await?;
    }
    sqlx::query("UPDATE user_buildings SET in_use = 0, x = 0, y = 0, rotate = 0 WHERE user_id = ?")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM user_roads WHERE user_id = ?")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE user_room_state SET is_reset = 1, last_reset_time = ? WHERE user_id = ?")
        .bind(ServerTime::now_ms())
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn get_room_state(pool: &SqlitePool, user_id: i64) -> Result<RoomState> {
    let state = sqlx::query_as::<_, (i32, String, String, bool)>(
        r#"
        SELECT room_level, room_theme_ids, room_skin_ids, have_fishing_bonus
        FROM user_room_state
        WHERE user_id = ?
        "#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    let Some((room_level, theme_ids, skin_ids, have_fishing_bonus)) = state else {
        return Ok(RoomState {
            room_level: default_room_level(),
            room_theme_ids: Vec::new(),
            room_skin_ids: Vec::new(),
            have_fishing_bonus: false,
        });
    };

    Ok(RoomState {
        room_level: if room_level > 0 {
            room_level
        } else {
            default_room_level()
        },
        room_theme_ids: serde_json::from_str(&theme_ids)?,
        room_skin_ids: serde_json::from_str(&skin_ids)?,
        have_fishing_bonus,
    })
}

pub async fn set_room_reset_state(pool: &SqlitePool, user_id: i64, is_reset: bool) -> Result<()> {
    let now = ServerTime::now_ms();
    sqlx::query(
        "INSERT INTO user_room_state (user_id, is_reset, last_reset_time)
         VALUES (?, ?, ?)
         ON CONFLICT(user_id) DO UPDATE SET
             is_reset = excluded.is_reset,
             last_reset_time = excluded.last_reset_time",
    )
    .bind(user_id)
    .bind(is_reset)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_room_level(pool: &SqlitePool, user_id: i64, room_level: i32) -> Result<()> {
    sqlx::query(
        "INSERT INTO user_room_state (user_id, room_level, last_reset_time)
         VALUES (?, ?, ?)
         ON CONFLICT(user_id) DO UPDATE SET room_level = excluded.room_level",
    )
    .bind(user_id)
    .bind(room_level)
    .bind(ServerTime::now_ms())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn level_up_room_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    current_level: i32,
    room_level: i32,
) -> Result<Option<Vec<RoomProductionLine>>> {
    let updated = sqlx::query(
        "UPDATE user_room_state
         SET room_level = ?, last_reset_time = ?
         WHERE user_id = ? AND room_level = ?",
    )
    .bind(room_level)
    .bind(ServerTime::now_ms())
    .bind(user_id)
    .bind(current_level)
    .execute(&mut **tx)
    .await?;
    if updated.rows_affected() != 1 {
        return Ok(None);
    }

    for line in config::configs::get()
        .production_line
        .iter()
        .filter(|line| line.need_room_level <= room_level)
    {
        sqlx::query(
            "UPDATE user_room_production_lines
             SET level = 1
             WHERE user_id = ? AND line_id = ? AND level = 0",
        )
        .bind(user_id)
        .bind(line.id)
        .execute(&mut **tx)
        .await?;
    }

    Ok(Some(
        sqlx::query_as(
            "SELECT line_id, formula_id, finish_count, next_finish_time, pause_time, level
             FROM user_room_production_lines
             WHERE user_id = ? AND level > 0
             ORDER BY line_id",
        )
        .bind(user_id)
        .fetch_all(&mut **tx)
        .await?,
    ))
}

pub async fn claim_room_theme_bonus(
    pool: &SqlitePool,
    user_id: i64,
    theme_id: i32,
) -> Result<bool> {
    let mut state = get_room_state(pool, user_id).await?;
    if state.room_theme_ids.contains(&theme_id) {
        return Ok(false);
    }

    state.room_theme_ids.push(theme_id);
    sqlx::query(
        "INSERT INTO user_room_state (user_id, room_theme_ids, last_reset_time)
         VALUES (?, ?, ?)
         ON CONFLICT(user_id) DO UPDATE SET room_theme_ids = excluded.room_theme_ids",
    )
    .bind(user_id)
    .bind(serde_json::to_string(&state.room_theme_ids)?)
    .bind(ServerTime::now_ms())
    .execute(pool)
    .await?;

    Ok(true)
}

pub async fn claim_room_theme_bonus_in_transaction(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    theme_id: i32,
) -> Result<bool> {
    let old_json: String =
        sqlx::query_scalar("SELECT room_theme_ids FROM user_room_state WHERE user_id = ?")
            .bind(user_id)
            .fetch_optional(&mut **tx)
            .await?
            .unwrap_or_else(|| "[]".to_string());
    let mut ids: Vec<i32> = serde_json::from_str(&old_json)?;
    if ids.contains(&theme_id) {
        return Ok(false);
    }
    ids.push(theme_id);
    let result = sqlx::query(
        "INSERT INTO user_room_state (user_id, room_theme_ids, last_reset_time)
         VALUES (?, ?, ?)
         ON CONFLICT(user_id) DO UPDATE SET room_theme_ids = excluded.room_theme_ids
         WHERE user_room_state.room_theme_ids = ?",
    )
    .bind(user_id)
    .bind(serde_json::to_string(&ids)?)
    .bind(ServerTime::now_ms())
    .bind(old_json)
    .execute(&mut **tx)
    .await?;
    Ok(result.rows_affected() == 1)
}

fn default_room_level() -> i32 {
    config::configs::get().initial_room_level()
}

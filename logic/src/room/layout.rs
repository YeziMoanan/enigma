use super::hero::room_heroes;
use super::plan::room_plan_building_degree;
use super::*;

impl RoomManager {
    pub async fn sync_room_tasks(
        &self,
        db: &SqlitePool,
        tables: &config::GameDB,
    ) -> Result<Vec<UserTask>, AppError> {
        task_db::ensure_tasks_for_type(db, self.player_id, task_db::TaskType::Room).await?;
        let (blocks, buildings) = if let Some(committed) =
            block_packages::committed_room_info(db, self.player_id).await?
        {
            (committed.infos, committed.building_infos)
        } else {
            (
                block_packages::get_blocks(db, self.player_id)
                    .await?
                    .into_iter()
                    .map(Into::into)
                    .collect(),
                buildings::get_placed_buildings(db, self.player_id)
                    .await?
                    .into_iter()
                    .map(Into::into)
                    .collect(),
            )
        };
        let room_level = block_packages::get_room_state(db, self.player_id)
            .await?
            .room_level;
        let block_count = blocks
            .iter()
            .filter(|block| block.block_id.unwrap_or_default() >= 0)
            .count() as i32;
        let building_count = buildings.len() as i32;
        let building_degree =
            room_plan_building_degree(db, tables, self.player_id, &blocks, &buildings).await?;

        let mut updated = Vec::new();
        for task in tables.online_room_tasks() {
            let progress = match task.listener_type.as_str() {
                "EditBlockCount" => block_count,
                "BuildingUseCount" => building_count,
                "BuildingDegree" => building_degree,
                "RoomLevel" => room_level,
                _ => continue,
            };
            if let Some(task) = task_db::sync_progress(
                db,
                self.player_id,
                task_db::TaskType::Room.id(),
                task.id,
                progress,
                task.max_progress,
            )
            .await?
            {
                updated.push(task);
            }
        }
        Ok(updated)
    }

    pub async fn room_confirm(&self, db: &SqlitePool) -> Result<RoomConfirmReply, AppError> {
        block_packages::commit_room_edit(db, self.player_id).await?;
        let state = block_packages::get_room_state(db, self.player_id).await?;

        Ok(RoomConfirmReply {
            infos: block_packages::get_blocks(db, self.player_id)
                .await?
                .into_iter()
                .map(Into::into)
                .collect(),
            building_infos: buildings::get_placed_buildings(db, self.player_id)
                .await?
                .into_iter()
                .map(Into::into)
                .collect(),
            formula_infos: room_ob::get_formulas(db, self.player_id)
                .await?
                .into_iter()
                .flat_map(|formula| formula.into_proto())
                .collect(),
            room_level: Some(state.room_level),
            room_hero_datas: room_heroes(db, self.player_id, &[]).await?,
            production_lines: room_ob::get_production_lines(db, self.player_id, &[])
                .await?
                .into_iter()
                .map(Into::into)
                .collect(),
        })
    }

    pub async fn room_revert(&self, db: &SqlitePool) -> Result<RoomRevertReply, AppError> {
        block_packages::revert_room_edit(db, self.player_id).await?;
        Ok(RoomRevertReply {
            infos: block_packages::get_blocks(db, self.player_id)
                .await?
                .into_iter()
                .map(Into::into)
                .collect(),
            block_packages: block_packages::get_block_packages(db, self.player_id)
                .await?
                .into_iter()
                .map(Into::into)
                .collect(),
            building_infos: buildings::get_placed_buildings(db, self.player_id)
                .await?
                .into_iter()
                .map(Into::into)
                .collect(),
        })
    }

    pub async fn generate_roads(
        &self,
        db: &SqlitePool,
        delete_ids: Vec<i32>,
        road_infos: Vec<RoadInfo>,
    ) -> Result<GenerateRoadReply, AppError> {
        if delete_ids.is_empty() && road_infos.is_empty() {
            return Ok(GenerateRoadReply {
                ids: Vec::new(),
                valid_road_infos: block_packages::get_roads(db, self.player_id)
                    .await?
                    .into_iter()
                    .map(Into::into)
                    .collect(),
            });
        }

        let owned_buildings = buildings::get_user_buildings(db, self.player_id).await?;
        let existing_roads = block_packages::get_roads(db, self.player_id).await?;
        let existing_ids: BTreeSet<_> = existing_roads.iter().map(|road| road.id).collect();
        let mut next_id = existing_ids.last().copied().unwrap_or_default() + 1;
        let mut roads = Vec::with_capacity(road_infos.len());

        for info in road_infos {
            if info.road_points.is_empty() {
                return Err(AppError::InvalidRequest);
            }
            let requested_id = info.id.unwrap_or_default();
            if requested_id > 0 && !existing_ids.contains(&requested_id) {
                return Err(AppError::InvalidRequest);
            }
            let id = if requested_id > 0 {
                requested_id
            } else {
                let id = next_id;
                next_id += 1;
                id
            };
            let building_uid = info.building_uid.unwrap_or_default();
            let building = (building_uid != 0)
                .then(|| {
                    owned_buildings
                        .iter()
                        .find(|building| building.uid == building_uid)
                        .ok_or(AppError::InvalidRequest)
                })
                .transpose()?;
            roads.push(database::models::game::block_packages::RoadInfo {
                user_id: self.player_id,
                id,
                from_type: info.from_type.unwrap_or_default(),
                to_type: info.to_type.unwrap_or_default(),
                road_points: serde_json::to_string(&info.road_points)?,
                critter_uid: info.critter_uid.unwrap_or_default(),
                building_uid,
                building_define_id: building.map_or(0, |building| building.define_id),
                skin_id: info.skin_id.unwrap_or_default(),
                block_clean_type: info.block_clean_type.unwrap_or_default(),
            });
        }

        let valid_road_infos =
            block_packages::edit_roads(db, self.player_id, &delete_ids, &roads).await?;

        Ok(GenerateRoadReply {
            ids: delete_ids,
            valid_road_infos: valid_road_infos.into_iter().map(Into::into).collect(),
        })
    }

    pub async fn delete_roads(
        &self,
        db: &SqlitePool,
        ids: Vec<i32>,
    ) -> Result<DeleteRoadReply, AppError> {
        if !ids.is_empty() {
            block_packages::edit_roads(db, self.player_id, &ids, &[]).await?;
        }
        Ok(DeleteRoadReply { ids })
    }

    pub async fn allot_road_critter(
        &self,
        db: &SqlitePool,
        id: i32,
        critter_uid: i64,
    ) -> Result<AllotCritterReply, AppError> {
        block_packages::allot_road_critter(db, self.player_id, id, critter_uid).await?;
        Ok(AllotCritterReply {
            id: Some(id),
            critter_uid: Some(critter_uid),
        })
    }

    pub async fn allot_road_vehicle(
        &self,
        db: &SqlitePool,
        id: i32,
        building_uid: i64,
        skin_id: i32,
    ) -> Result<AllotVehicleReply, AppError> {
        let building_define_id =
            block_packages::allot_road_vehicle(db, self.player_id, id, building_uid, skin_id)
                .await?;
        Ok(AllotVehicleReply {
            id: Some(id),
            building_uid: Some(building_uid),
            skin_id: Some(skin_id),
            building_define_id: Some(building_define_id),
        })
    }

    pub async fn other_room_ob_info(
        &self,
        db: &SqlitePool,
        target_uid: i64,
    ) -> Result<GetOtherRoomObInfoReply, AppError> {
        let state = block_packages::get_room_state(db, target_uid).await?;
        let committed = block_packages::committed_room_info(db, target_uid).await?;
        let share_code = room_plan::get_room_plan(db, target_uid, 0)
            .await?
            .and_then(|plan| plan.share_code)
            .unwrap_or_default();

        Ok(GetOtherRoomObInfoReply {
            infos: if let Some(snapshot) = &committed {
                snapshot.infos.clone()
            } else {
                block_packages::get_blocks(db, target_uid)
                    .await?
                    .into_iter()
                    .map(Into::into)
                    .collect()
            },
            building_infos: if let Some(snapshot) = &committed {
                snapshot.building_infos.clone()
            } else {
                buildings::get_placed_buildings(db, target_uid)
                    .await?
                    .into_iter()
                    .map(Into::into)
                    .collect()
            },
            target_uid: Some(target_uid),
            room_level: Some(state.room_level),
            room_hero_datas: room_heroes(db, target_uid, &[]).await?,
            production_lines: room_ob::get_production_lines(db, target_uid, &[])
                .await?
                .into_iter()
                .map(Into::into)
                .collect(),
            share_code: Some(share_code),
            skins: room_ob::get_skins(db, target_uid)
                .await?
                .into_iter()
                .map(Into::into)
                .collect(),
            road_infos: if let Some(snapshot) = committed {
                snapshot.road_infos
            } else {
                block_packages::get_roads(db, target_uid)
                    .await?
                    .into_iter()
                    .map(Into::into)
                    .collect()
            },
        })
    }

    pub async fn read_room_log_new(
        &self,
        db: &SqlitePool,
        index: Vec<i32>,
    ) -> Result<ReadRoomLogNewReply, AppError> {
        room_plan::read_room_logs(db, self.player_id, &index).await?;
        Ok(ReadRoomLogNewReply { index })
    }

    pub async fn room_theme_collection_bonus(
        &self,
        db: &SqlitePool,
        tables: &config::GameDB,
        theme_id: i32,
    ) -> Result<RoomReward<GetRoomThemeCollectionBonusReply>, AppError> {
        let theme = tables
            .room_theme
            .get(theme_id)
            .ok_or(AppError::InvalidRequest)?;
        let mut tx = db.begin_with("BEGIN IMMEDIATE").await?;
        let claimed = block_packages::claim_room_theme_bonus_in_transaction(
            &mut tx,
            self.player_id,
            theme_id,
        )
        .await?;
        let reward_set = if claimed {
            reward::parse(&theme.collection_bonus)
        } else {
            reward::RewardSet::default()
        };
        let material_changes = reward_set.material_changes();
        let rewards = if reward_set.is_empty() {
            reward::AppliedRewards::default()
        } else {
            reward::apply_in_transaction(&mut tx, db, self.player_id, reward_set).await?
        };
        tx.commit().await?;

        Ok(RoomReward {
            reply: GetRoomThemeCollectionBonusReply { id: Some(theme_id) },
            rewards,
            material_changes,
        })
    }
}

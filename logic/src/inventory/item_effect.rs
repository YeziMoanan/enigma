use super::*;

pub(super) async fn use_equipment_level_item(
    db: &SqlitePool,
    player_id: i64,
    item_id: u32,
    target_uid: i64,
    effect: &str,
) -> Result<(), AppError> {
    let tables = config::configs::get();
    let equip = equipment_db::get_equipment_by_uid(db, player_id, target_uid)
        .await
        .map_err(|_| AppError::InvalidRequest)?;
    if equip.count <= 0 {
        return Err(AppError::InvalidRequest);
    }
    let equip_config = tables
        .equip
        .get(equip.equip_id)
        .ok_or(AppError::InvalidRequest)?;
    if !equipment_level_item_allows(effect, equip_config) {
        return Err(AppError::InvalidRequest);
    }
    let (max_break, max_level) = tables
        .max_equip_progression(equip_config.rare)
        .map(|row| (row.break_level, row.level))
        .ok_or(AppError::InvalidRequest)?;
    if equip.level >= max_level && equip.break_lv >= max_break {
        return Err(AppError::InvalidRequest);
    }

    if !equipment_db::consume_item_and_max_equipment(
        db, player_id, item_id, target_uid, max_level, max_break,
    )
    .await?
    {
        return Err(AppError::InsufficientItems);
    }
    Ok(())
}

fn equipment_level_item_allows(effect: &str, equip: &config::equip::Equip) -> bool {
    let mut parts = effect.split('|');
    match parts
        .next()
        .and_then(|part| part.parse::<i32>().ok())
        .and_then(EquipmentLevelEffectType::from_id)
    {
        Some(EquipmentLevelEffectType::All) => {
            let rare = parts
                .next()
                .and_then(|part| part.split('#').next())
                .and_then(|part| part.parse::<i32>().ok())
                .unwrap_or(5);
            equip.rare == rare && equip.is_exp_equip == 0 && equip.is_sp_refine == 0
        }
        Some(EquipmentLevelEffectType::Specify) => {
            parts.any(|part| part.parse::<i32>().ok() == Some(equip.id))
        }
        _ => false,
    }
}

pub(super) fn target_item_rewards(effect: &str, target_id: i32) -> Option<reward::RewardSet> {
    let reward_segments = effect
        .split('|')
        .map(reward::parse)
        .filter(|rewards| !rewards.is_empty())
        .collect::<Vec<_>>();
    if !reward_segments.is_empty() {
        return reward_segments
            .into_iter()
            .find(|rewards| reward_has_id(rewards, target_id))
            .or_else(|| {
                effect
                    .split('|')
                    .nth(target_id as usize)
                    .map(reward::parse)
                    .filter(|rewards| !rewards.is_empty())
            });
    }

    if target_id > 0
        && effect_ids(effect).contains(&target_id)
        && config::configs::get().character.get(target_id).is_some()
    {
        let mut selected = reward::RewardSet::default();
        selected.heroes.push((target_id, 1));
        return Some(selected);
    }

    None
}

fn reward_has_id(rewards: &reward::RewardSet, target_id: i32) -> bool {
    rewards.items.iter().any(|(id, _)| *id == target_id as u32)
        || rewards.currencies.iter().any(|(id, _)| *id == target_id)
        || rewards
            .block_packages
            .iter()
            .any(|(id, _)| *id == target_id)
        || rewards.heroes.iter().any(|(id, _)| *id == target_id)
        || rewards.skins.iter().any(|(id, _)| *id == target_id)
        || rewards.equips.iter().any(|(id, _)| *id == target_id)
        || rewards.power_items.iter().any(|(id, _)| *id == target_id)
        || rewards
            .room_buildings
            .iter()
            .any(|(id, _)| *id == target_id)
        || rewards
            .special_blocks
            .iter()
            .any(|(id, _)| *id == target_id)
        || rewards.antiques.iter().any(|(id, _)| *id == target_id)
        || rewards.insight_items.iter().any(|(id, _)| *id == target_id)
        || rewards.bp_scores.iter().any(|(id, _)| *id == target_id)
}

pub(super) fn effect_ids(effect: &str) -> Vec<i32> {
    effect
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|part| part.parse().ok())
        .collect()
}

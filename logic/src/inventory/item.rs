use super::*;
use rand::prelude::IndexedRandom;

pub(super) async fn use_items(
    db: &SqlitePool,
    player_id: i64,
    entries: Vec<M2qEntry>,
    target_id: Option<u64>,
) -> Result<
    (
        UseItemReply,
        reward::AppliedRewards,
        Vec<u32>,
        Vec<(u32, u32, i32)>,
    ),
    AppError,
> {
    if entries.len() == 1 {
        let entry = &entries[0];
        let item_id = entry.material_id.ok_or(AppError::InvalidRequest)?;
        let item = config::configs::get()
            .item
            .get(item_id as i32)
            .ok_or(AppError::InvalidRequest)?;
        if item.sub_type == ItemSubType::DestinyStoneUp as i32
            || item.sub_type == ItemSubType::NewDestinyStoneUp as i32
        {
            return use_destiny_stone_up(db, player_id, entries, target_id).await;
        }
        if item.sub_type == ItemSubType::HeroExpBox as i32 {
            return use_hero_exp_box(db, player_id, entries, target_id).await;
        }
    }

    if let Some(target_uid) = target_id.filter(|target_id| *target_id != 0) {
        if entries.len() != 1 {
            return Err(AppError::InvalidRequest);
        }
        let entry = &entries[0];
        let item_id = entry.material_id.ok_or(AppError::InvalidRequest)?;
        let quantity = entry.quantity.unwrap_or(1);
        let item = config::configs::get()
            .item
            .get(item_id as i32)
            .ok_or(AppError::InvalidRequest)?;
        if item.sub_type == ItemSubType::EquipmentLevelUp as i32 {
            if quantity != 1 {
                return Err(AppError::InvalidRequest);
            }
            let target_uid = i64::try_from(target_uid).map_err(|_| AppError::InvalidRequest)?;
            use_equipment_level_item(db, player_id, item_id, target_uid, &item.effect).await?;
            let changed = reward::AppliedRewards {
                equip_uids: vec![target_uid],
                ..Default::default()
            };
            return Ok((
                UseItemReply {
                    entry: entries,
                    target_id,
                },
                changed,
                vec![item_id],
                Vec::new(),
            ));
        }
    }

    let mut material_changes = Vec::new();
    let mut costs = reward::RewardSet::default();
    let mut rewards = reward::RewardSet::default();

    for entry in &entries {
        let item_id = entry.material_id.ok_or(AppError::InvalidRequest)?;
        let quantity = entry.quantity.unwrap_or(1).max(1);
        let item_rewards = item_rewards(item_id as i32, quantity, target_id)?;
        material_changes.extend(item_rewards.material_changes());
        costs.items.push((item_id, quantity));
        rewards.extend(item_rewards);
    }

    let mut tx = db.begin().await?;
    let consumed = reward::consume(&mut tx, player_id, &costs).await?;
    let changed = reward::apply_in_transaction(&mut tx, db, player_id, rewards).await?;
    tx.commit().await?;

    Ok((
        UseItemReply {
            entry: entries,
            target_id,
        },
        changed,
        consumed.item_ids,
        material_changes,
    ))
}

pub(super) async fn use_insight_item(
    db: &SqlitePool,
    player_id: i64,
    uid: i64,
    hero_id: i32,
) -> Result<(UseInsightItemReply, i32), AppError> {
    let item = items::get_insight_item_by_uid(db, player_id, uid)
        .await?
        .ok_or(AppError::InvalidRequest)?;
    let item_id = item.item_id;

    if item.quantity <= 0 || item.expire_time <= ServerTime::now_sec_i32() {
        return Err(AppError::InvalidRequest);
    }

    let config = config::configs::get()
        .insight_item
        .get(item_id)
        .ok_or(AppError::InvalidRequest)?;
    let target_rank = config.hero_rank + 1;
    let target_level = config
        .effect
        .split('#')
        .nth(1)
        .and_then(|level| level.parse().ok())
        .unwrap_or(1);

    let heroes = UserHeroModel::new(player_id, db.clone());
    let current = heroes.get(hero_id).await?;
    let character = config::configs::get()
        .character
        .get(hero_id)
        .ok_or(AppError::InvalidRequest)?;
    if current.record.rank >= target_rank
        || !config
            .hero_rares
            .split('#')
            .filter_map(|rare| rare.parse::<i32>().ok())
            .any(|rare| rare == character.rare + 1)
    {
        return Err(AppError::InvalidRequest);
    }
    if !heroes
        .apply_insight_item(InsightUpgrade {
            item_uid: uid,
            item_id,
            hero_id,
            current_rank: current.record.rank,
            current_level: current.record.level,
            target_rank,
            target_level,
        })
        .await?
    {
        return Err(AppError::InvalidRequest);
    }

    Ok((
        UseInsightItemReply {
            uid: Some(uid),
            hero_id: Some(hero_id),
        },
        item_id,
    ))
}

pub(super) async fn mark_read_sub_type21(
    db: &SqlitePool,
    player_id: i64,
    item_id: i32,
) -> Result<MarkReadSubType21Reply, AppError> {
    red_dots::hide_red_dot_infos(
        db,
        player_id,
        RedDotId::PlayerChangeBgItemNew.id(),
        vec![item_id],
    )
    .await?;

    Ok(MarkReadSubType21Reply {
        item_id: Some(item_id),
    })
}

pub(super) fn item_rewards(
    item_id: i32,
    quantity: i32,
    target_id: Option<u64>,
) -> Result<reward::RewardSet, AppError> {
    let item = config::configs::get()
        .item
        .get(item_id)
        .ok_or(AppError::InvalidRequest)?;

    let target_id = target_id.and_then(|id| i32::try_from(id).ok());
    let target = target_id.filter(|id| *id != 0);
    let mut rewards = match item.sub_type {
        subtype if subtype == ItemSubType::SpecifiedGift as i32 => target_id
            .and_then(|target| target_item_rewards(&item.effect, target))
            .ok_or(AppError::InvalidRequest)?,
        subtype if subtype == ItemSubType::OptionalGift as i32 => target
            .and_then(|target| optional_gift_rewards(item, target))
            .ok_or(AppError::InvalidRequest)?,
        subtype if subtype == ItemSubType::SkinSelectGift as i32 => target
            .and_then(|target| skin_select_rewards(&item.effect, target))
            .ok_or(AppError::InvalidRequest)?,
        subtype if subtype == ItemSubType::HeroExpBox as i32 => {
            return Err(AppError::InvalidRequest);
        }
        subtype if subtype == ItemSubType::EquipmentLevelUp as i32 && target.is_none() => {
            reward::parse(&item.effect)
        }
        _ if target.is_some() => {
            target_item_rewards(&item.effect, target.unwrap()).ok_or(AppError::InvalidRequest)?
        }
        _ => bonus_or_inline_rewards(&item.effect).ok_or(AppError::InvalidRequest)?,
    };

    rewards.scale(quantity);

    if rewards.is_empty() {
        Err(AppError::InvalidRequest)
    } else {
        Ok(rewards)
    }
}

async fn use_hero_exp_box(
    db: &SqlitePool,
    player_id: i64,
    entries: Vec<M2qEntry>,
    target_id: Option<u64>,
) -> Result<
    (
        UseItemReply,
        reward::AppliedRewards,
        Vec<u32>,
        Vec<(u32, u32, i32)>,
    ),
    AppError,
> {
    let entry = &entries[0];
    let box_id = entry.material_id.ok_or(AppError::InvalidRequest)?;
    if entry.quantity.unwrap_or(1) != 1 {
        return Err(AppError::InvalidRequest);
    }
    let item = config::configs::get()
        .item
        .get(box_id as i32)
        .ok_or(AppError::InvalidRequest)?;
    let mut parts = item.effect.split('|');
    let key_count = parts
        .next()
        .and_then(|part| part.parse::<i32>().ok())
        .filter(|count| *count > 0)
        .ok_or(AppError::InvalidRequest)?;
    let overflow = parts.next().ok_or(AppError::InvalidRequest)?;
    let hero_ids = parts.next().ok_or(AppError::InvalidRequest)?;
    let key_id = config::configs::get()
        .item
        .all()
        .iter()
        .find(|row| row.sub_type == ItemSubType::HeroExpBoxKey as i32)
        .map(|row| row.id as u32)
        .ok_or(AppError::InvalidRequest)?;

    let target = target_id
        .and_then(|id| i32::try_from(id).ok())
        .unwrap_or_default();
    let rewards = if target == 0 {
        reward::parse(overflow)
    } else {
        if !effect_ids(hero_ids).contains(&target) {
            return Err(AppError::InvalidRequest);
        }
        let hero_data = UserHeroModel::new(player_id, db.clone())
            .get(target)
            .await?;
        let duplicate_item_id = hero::duplicate_item_id(target)?;
        let owned_duplicates = items::get_item(db, player_id, duplicate_item_id)
            .await?
            .map(|item| item.quantity)
            .unwrap_or_default();
        if hero_data.record.ex_skill_level >= 5
            || hero_data.record.ex_skill_level + owned_duplicates >= 5
        {
            return Err(AppError::InvalidRequest);
        }
        reward::RewardSet {
            items: vec![(duplicate_item_id, 1)],
            ..Default::default()
        }
    };
    if rewards.is_empty() {
        return Err(AppError::InvalidRequest);
    }
    let material_changes = rewards.material_changes();
    let costs = reward::RewardSet {
        items: vec![(box_id, 1), (key_id, key_count)],
        ..Default::default()
    };
    let mut tx = db.begin().await?;
    let consumed = reward::consume(&mut tx, player_id, &costs).await?;
    let changed = reward::apply_in_transaction(&mut tx, db, player_id, rewards).await?;
    tx.commit().await?;

    Ok((
        UseItemReply {
            entry: entries,
            target_id,
        },
        changed,
        consumed.item_ids,
        material_changes,
    ))
}

async fn use_destiny_stone_up(
    db: &SqlitePool,
    player_id: i64,
    entries: Vec<M2qEntry>,
    target_id: Option<u64>,
) -> Result<
    (
        UseItemReply,
        reward::AppliedRewards,
        Vec<u32>,
        Vec<(u32, u32, i32)>,
    ),
    AppError,
> {
    let entry = &entries[0];
    let item_id = entry.material_id.ok_or(AppError::InvalidRequest)?;
    if entry.quantity.unwrap_or(1) != 1 {
        return Err(AppError::InvalidRequest);
    }
    let target_stone = target_id
        .and_then(|id| i32::try_from(id).ok())
        .filter(|id| *id > 0)
        .ok_or(AppError::InvalidRequest)?;
    let item = config::configs::get()
        .item
        .get(item_id as i32)
        .ok_or(AppError::InvalidRequest)?;
    let parts = item.effect.split('|').collect::<Vec<_>>();
    let target_progress = parts
        .first()
        .map(|part| effect_ids(part))
        .unwrap_or_default();
    let [target_rank, target_level] = target_progress.as_slice() else {
        return Err(AppError::InvalidRequest);
    };
    let ignored = parts
        .get(1)
        .map(|part| effect_ids(part))
        .unwrap_or_default();
    if ignored.contains(&target_stone) {
        return Err(AppError::InvalidRequest);
    }

    let matched_heroes = config::configs::get()
        .character
        .all()
        .iter()
        .filter(|character| hero::destiny_stones(character.id).contains(&target_stone))
        .map(|character| character.id)
        .collect::<Vec<_>>();
    let [hero_id] = matched_heroes.as_slice() else {
        return Err(AppError::InvalidRequest);
    };
    if let Some(allowed) = parts.get(2).map(|part| effect_ids(part))
        && !allowed.contains(hero_id)
    {
        return Err(AppError::InvalidRequest);
    }

    let heroes = UserHeroModel::new(player_id, db.clone());
    let current = heroes.get(*hero_id).await?;
    if !hero::destiny_available(*hero_id, current.record.rank, current.record.level) {
        return Err(AppError::InvalidRequest);
    }
    let slots_id = config::configs::get()
        .character_destiny(*hero_id)
        .map(|row| row.slots_id)
        .ok_or(AppError::InvalidRequest)?;
    if config::configs::get()
        .character_destiny_slot(slots_id, *target_rank, *target_level)
        .is_none()
        || (current.record.destiny_rank, current.record.destiny_level)
            > (*target_rank, *target_level)
    {
        return Err(AppError::InvalidRequest);
    }
    let needs_progress = (current.record.destiny_rank, current.record.destiny_level)
        != (*target_rank, *target_level);
    let needs_unlock = !current.destiny_stone_unlocks.contains(&target_stone);
    if !needs_progress && !needs_unlock {
        return Err(AppError::InvalidRequest);
    }

    let mut tx = db.begin().await?;
    let consumed = reward::consume(
        &mut tx,
        player_id,
        &reward::RewardSet {
            items: vec![(item_id, 1)],
            ..Default::default()
        },
    )
    .await?;
    if needs_progress
        && !heroes
            .update_destiny_progress_in_transaction(
                &mut tx,
                *hero_id,
                current.record.destiny_rank,
                current.record.destiny_level,
                *target_rank,
                *target_level,
            )
            .await?
    {
        return Err(AppError::InvalidRequest);
    }
    if needs_unlock
        && !heroes
            .unlock_destiny_stone_in_transaction(&mut tx, *hero_id, target_stone)
            .await?
    {
        return Err(AppError::InvalidRequest);
    }
    tx.commit().await?;

    Ok((
        UseItemReply {
            entry: entries,
            target_id,
        },
        reward::AppliedRewards {
            hero_ids: vec![*hero_id],
            ..Default::default()
        },
        consumed.item_ids,
        Vec::new(),
    ))
}

fn bonus_or_inline_rewards(effect: &str) -> Option<reward::RewardSet> {
    let inline = reward::parse(effect);
    if !inline.is_empty() {
        return Some(inline);
    }
    let bonus_id = effect.parse::<i32>().ok()?;
    let configured = reward::parse_bonus(bonus_id);
    if !configured.is_empty() {
        return Some(configured);
    }
    server_only_bonus_rewards(bonus_id)
}

fn server_only_bonus_rewards(bonus_id: i32) -> Option<reward::RewardSet> {
    let fixed_currency = match bonus_id {
        1001009 => Some((3, 100_000)),
        1001010 => Some((3, 500_000)),
        1001011 => Some((5, 100_000)),
        1001012 => Some((5, 500_000)),
        _ => None,
    };
    if let Some((currency_id, amount)) = fixed_currency {
        return Some(reward::RewardSet {
            currencies: vec![(currency_id, amount)],
            ..Default::default()
        });
    }

    let (rare, resonance_only) = match bonus_id {
        1001003 => (1, false),
        1001004 => (2, false),
        1001005 => (5, true),
        1001006 => (3, false),
        1001007 => (4, false),
        1001008 => (5, false),
        _ => return None,
    };
    let candidates = config::configs::get()
        .item
        .all()
        .iter()
        .filter(|item| {
            item.rare == rare
                && item.is_show == 1
                && if resonance_only {
                    item.sub_type == 12
                } else {
                    item.sub_type == 11 || item.sub_type == 12
                }
        })
        .collect::<Vec<_>>();
    let selected = candidates.choose(&mut rand::rng())?;
    Some(reward::RewardSet {
        items: vec![(selected.id as u32, 1)],
        ..Default::default()
    })
}

fn optional_gift_rewards(item: &config::item::Item, target_id: i32) -> Option<reward::RewardSet> {
    let mut parts = item.effect.split('|');
    let selector = effect_ids(parts.next()?);
    let [sub_type, configured_rare, ..] = selector.as_slice() else {
        return None;
    };
    let ignored = parts.next().map(effect_ids).unwrap_or_default();
    let target = config::configs::get().item.get(target_id)?;
    let rare = if *configured_rare == 0 {
        item.rare
    } else {
        *configured_rare
    };
    if target.sub_type != *sub_type || target.rare != rare || ignored.contains(&target_id) {
        return None;
    }
    Some(reward::RewardSet {
        items: vec![(target_id as u32, 1)],
        ..Default::default()
    })
}

fn skin_select_rewards(effect: &str, target_id: i32) -> Option<reward::RewardSet> {
    let allowed = effect.split('|').next().map(effect_ids)?;
    if !allowed.contains(&target_id) || config::configs::get().skin.get(target_id).is_none() {
        return None;
    }
    Some(reward::RewardSet {
        skins: vec![(target_id, 1)],
        ..Default::default()
    })
}

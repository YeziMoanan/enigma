use super::*;
use database::db::game::{currencies, items};
use rand::prelude::IndexedRandom;
use sonettobuf::{Act197ExploreReply, Act197GainInfo, Act197RummageReply, Get197InfoReply};

enum Act197GainId {
    Big = 1,
}

enum Act197FindType {
    All = 2,
}

pub struct Act197Claim {
    pub reply: Act197RummageReply,
    pub rewards: reward::AppliedRewards,
    pub material_changes: Vec<(u32, u32, i32)>,
}

pub struct Act197Explore {
    pub reply: Act197ExploreReply,
    pub rewards: reward::AppliedRewards,
    pub material_changes: Vec<(u32, u32, i32)>,
}

#[derive(Clone, Copy)]
struct MaterialAmount {
    material_type: u32,
    id: u32,
    count: i32,
}

pub async fn act197_info(
    db: &SqlitePool,
    player_id: i64,
    activity_id: Option<i32>,
) -> Result<Get197InfoReply, AppError> {
    let activity_id = resolve_activity_id(activity_id)?;
    let states = activity_state::get(
        db,
        player_id,
        activity_id,
        ActivityStateKind::Act197PoolGain,
    )
    .await?;

    Ok(Get197InfoReply {
        activity_id: Some(activity_id),
        has_gain: gain_infos(&states),
    })
}

pub async fn act197_rummage(
    db: &SqlitePool,
    player_id: i64,
    activity_id: Option<i32>,
    pool_id: Option<i32>,
) -> Result<Act197Claim, AppError> {
    let activity_id = resolve_activity_id(activity_id)?;
    let pool_id = pool_id.ok_or(AppError::InvalidRequest)?;
    let config = activity_config(activity_id)?;
    let cost = parse_material(&config.rummage_consume).ok_or(AppError::InvalidRequest)?;
    let states = activity_state::get(
        db,
        player_id,
        activity_id,
        ActivityStateKind::Act197PoolGain,
    )
    .await?;

    if !pool_is_open(pool_id, &states) {
        return Err(AppError::InvalidRequest);
    }

    let claimed = claimed_gain_ids(&states, pool_id);
    let rows = config::configs::get()
        .activity197_pool
        .iter()
        .filter(|row| row.activity_id == activity_id && row.pool_id == pool_id)
        .filter(|row| !claimed.contains(&row.index))
        .collect::<Vec<_>>();
    let row = rows
        .choose(&mut rand::rng())
        .ok_or(AppError::InvalidRequest)?;

    consume_material(db, player_id, cost, 1).await?;

    let parsed = reward::parse(&row.bonus);
    let mut material_changes = vec![(cost.material_type, cost.id, -cost.count)];
    material_changes.extend(parsed.material_changes());
    let mut rewards = reward::apply(db, player_id, parsed).await?;
    add_consumed_push_ids(&mut rewards, cost, 1);

    let mut claimed = claimed;
    claimed.push(row.index);
    claimed.sort_unstable();
    claimed.dedup();
    let ext = serde_json::to_string(&claimed)?;
    activity_state::set(
        db,
        player_id,
        activity_id,
        ActivityStateSet {
            kind: ActivityStateKind::Act197PoolGain,
            entry_id: pool_id,
            state: 1,
            progress: 0,
            ext: &ext,
        },
    )
    .await?;

    Ok(Act197Claim {
        reply: Act197RummageReply {
            activity_id: Some(activity_id),
            pool_id: Some(pool_id),
            id: Some(row.index),
        },
        rewards,
        material_changes,
    })
}

pub async fn act197_explore(
    db: &SqlitePool,
    player_id: i64,
    activity_id: Option<i32>,
    find_type: Option<i32>,
) -> Result<Act197Explore, AppError> {
    let activity_id = resolve_activity_id(activity_id)?;
    let config = activity_config(activity_id)?;
    let cost = parse_material(&config.explore_consume).ok_or(AppError::InvalidRequest)?;
    let gain = parse_material(&config.explore_item).ok_or(AppError::InvalidRequest)?;
    let times = if find_type == Some(Act197FindType::All as i32) {
        material_quantity(db, player_id, cost).await? / cost.count
    } else {
        1
    };
    if times <= 0 {
        return Err(insufficient_error(cost));
    }

    consume_material(db, player_id, cost, times).await?;

    let parsed = reward_set_from_material(gain, times);
    let mut material_changes = vec![(cost.material_type, cost.id, -(cost.count * times))];
    material_changes.extend(parsed.material_changes());
    let mut rewards = reward::apply(db, player_id, parsed).await?;
    add_consumed_push_ids(&mut rewards, cost, times);

    Ok(Act197Explore {
        reply: Act197ExploreReply {
            activity_id: Some(activity_id),
        },
        rewards,
        material_changes,
    })
}

fn resolve_activity_id(activity_id: Option<i32>) -> Result<i32, AppError> {
    activity_id
        .or_else(|| {
            config::configs::get()
                .activity197
                .iter()
                .map(|row| row.activity_id)
                .max()
        })
        .ok_or(AppError::InvalidRequest)
}

fn activity_config(
    activity_id: i32,
) -> Result<&'static config::activity197::Activity197, AppError> {
    config::configs::get()
        .activity197
        .iter()
        .find(|row| row.activity_id == activity_id)
        .ok_or(AppError::InvalidRequest)
}

fn gain_infos(states: &activity_state::ActivityStates) -> Vec<Act197GainInfo> {
    let mut infos = states
        .iter()
        .filter_map(|(pool_id, (state, _, ext))| {
            if *state == 0 {
                return None;
            }

            Some(Act197GainInfo {
                pool_id: Some(*pool_id),
                gain_ids: parse_gain_ids(ext),
            })
        })
        .collect::<Vec<_>>();
    infos.sort_by_key(|info| info.pool_id.unwrap_or_default());
    infos
}

fn claimed_gain_ids(states: &activity_state::ActivityStates, pool_id: i32) -> Vec<i32> {
    states
        .get(&pool_id)
        .filter(|(state, _, _)| *state != 0)
        .map(|(_, _, ext)| parse_gain_ids(ext))
        .unwrap_or_default()
}

fn parse_gain_ids(ext: &str) -> Vec<i32> {
    let mut ids = serde_json::from_str::<Vec<i32>>(ext).unwrap_or_default();
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn pool_is_open(pool_id: i32, states: &activity_state::ActivityStates) -> bool {
    pool_id == 1 || claimed_gain_ids(states, pool_id - 1).contains(&(Act197GainId::Big as i32))
}

fn parse_material(value: &str) -> Option<MaterialAmount> {
    let fields = value
        .split('#')
        .filter_map(|field| field.parse::<i32>().ok())
        .collect::<Vec<_>>();
    let [material_type, id, count] = fields.as_slice() else {
        return None;
    };

    Some(MaterialAmount {
        material_type: *material_type as u32,
        id: *id as u32,
        count: *count,
    })
}

async fn consume_material(
    db: &SqlitePool,
    player_id: i64,
    material: MaterialAmount,
    times: i32,
) -> Result<(), AppError> {
    let amount = material.count * times;
    let ok = if material.material_type == reward::RewardMaterialType::Currency.id() {
        currencies::remove_currency(db, player_id, material.id as i32, amount).await?
    } else if material.material_type == reward::RewardMaterialType::Item.id() {
        items::remove_item_quantity(db, player_id, material.id, amount).await?
    } else {
        return Err(AppError::InvalidRequest);
    };

    if ok {
        Ok(())
    } else {
        Err(insufficient_error(material))
    }
}

async fn material_quantity(
    db: &SqlitePool,
    player_id: i64,
    material: MaterialAmount,
) -> Result<i32, AppError> {
    if material.material_type == reward::RewardMaterialType::Currency.id() {
        return Ok(currencies::get_currency(db, player_id, material.id as i32)
            .await?
            .map(|currency| currency.protocol_quantity())
            .unwrap_or(0));
    }

    if material.material_type == reward::RewardMaterialType::Item.id() {
        return Ok(items::get_item(db, player_id, material.id)
            .await?
            .map(|item| item.quantity)
            .unwrap_or(0));
    }

    Err(AppError::InvalidRequest)
}

fn insufficient_error(material: MaterialAmount) -> AppError {
    if material.material_type == reward::RewardMaterialType::Currency.id() {
        AppError::InsufficientCurrency
    } else {
        AppError::InsufficientItems
    }
}

fn reward_set_from_material(material: MaterialAmount, times: i32) -> reward::RewardSet {
    let count = material.count * times;
    if material.material_type == reward::RewardMaterialType::Currency.id() {
        reward::RewardSet {
            currencies: vec![(material.id as i32, count)],
            ..Default::default()
        }
    } else if material.material_type == reward::RewardMaterialType::Item.id() {
        reward::RewardSet {
            items: vec![(material.id, count)],
            ..Default::default()
        }
    } else {
        reward::RewardSet::default()
    }
}

fn add_consumed_push_ids(
    rewards: &mut reward::AppliedRewards,
    material: MaterialAmount,
    times: i32,
) {
    if material.material_type == reward::RewardMaterialType::Currency.id() {
        rewards
            .currency_ids
            .push((material.id as i32, -(material.count * times)));
    } else if material.material_type == reward::RewardMaterialType::Item.id() {
        rewards.item_ids.push(material.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn act197_uses_pool_id_as_state_entry() {
        let mut states = activity_state::ActivityStates::new();
        assert!(pool_is_open(1, &states));
        assert!(!pool_is_open(2, &states));

        states.insert(1, (1, 0, "[1]".to_string()));
        assert_eq!(claimed_gain_ids(&states, 1), vec![1]);
        assert!(pool_is_open(2, &states));
    }
}

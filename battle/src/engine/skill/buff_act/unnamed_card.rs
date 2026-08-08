use crate::engine::{
    event::payload::BattleEvent,
    manager::{
        BattleManagers,
        buff::{BuffCommand, BuffConsume, BuffSelector, DepletedBuff},
        card::{CardCommand, CardInsertUnnamed, CardUpdateUnnamed},
    },
    skill::{
        action::{SkillExecutionMode, SkillPhase},
        effect::SkillEffectCatalog,
        rule::output::{BattleCommand, RuleOp},
        subscriber::BuffActSubscriber,
    },
};

use super::registry::BuffActKind;

pub fn supports_add(args: &[i32]) -> bool {
    matches!(args, [skill_id, max_from_right, fragment_buff_id, max_consume]
        if *skill_id > 0 && *max_from_right > 0 && *fragment_buff_id > 0 && *max_consume > 0)
}

pub fn supports_charging(args: &[i32]) -> bool {
    args.len() == 8 && args.iter().all(|value| *value > 0)
}

pub fn round_start_rule_ops(
    managers: &BattleManagers,
    subscriber: &BuffActSubscriber,
    event: &BattleEvent,
) -> Option<Vec<super::BuffActRuleOp>> {
    if !super::subscriber_is_kind(subscriber, BuffActKind::UnnamedAddSpCard)
        || !matches!(
            event,
            BattleEvent::Kind(crate::engine::event::kind::EventKind::RoundStartCard)
        )
    {
        return None;
    }
    let [skill_id, max_from_right, fragment_buff_id, max_consume] = subscriber.args.as_slice()
    else {
        return None;
    };
    let origin = super::command_origin(subscriber)?;
    let hand = managers.card.hand();
    let existing = hand
        .iter()
        .position(crate::engine::manager::card::unnamed::is_unnamed);
    let insert_index = hand
        .len()
        .saturating_sub(usize::try_from(max_from_right.saturating_sub(1)).ok()?);
    let mut card_index = existing.unwrap_or(insert_index);
    let mut hand_len = hand.len();
    let mut ops = Vec::new();

    if existing.is_none() {
        ops.push(super::BuffActRuleOp::subscriber_from_owner(
            RuleOp::Command(BattleCommand::Card(CardCommand::InsertUnnamed(
                CardInsertUnnamed {
                    origin,
                    owner_uid: subscriber.owner_uid,
                    skill_id: *skill_id,
                    index: insert_index,
                },
            ))),
        ));
        hand_len = hand_len.saturating_add(1);
    }

    let fragments = managers
        .buff
        .buff_id_amount(subscriber.owner_uid, *fragment_buff_id)
        .min(*max_consume)
        .max(0);
    let first_insight_boost = existing.is_none()
        && managers
            .buff
            .active_features(&managers.hp)
            .into_iter()
            .any(|feature| {
                feature.owner_uid == subscriber.owner_uid
                    && feature.buff_id == 31471009
                    && feature.act_id() == Some(1136)
            });
    if fragments > 0 {
        ops.push(super::BuffActRuleOp::subscriber_from_owner(
            RuleOp::Command(BattleCommand::Buff(BuffCommand::Consume(BuffConsume {
                origin,
                target_uid: subscriber.owner_uid,
                selector: BuffSelector::ExactId(*fragment_buff_id),
                amount: fragments,
                depleted: DepletedBuff::Remove,
            }))),
        ));
    }
    let move_distance = fragments.saturating_add(if first_insight_boost { 8 } else { 0 });
    let strengthen_count = fragments
        .saturating_mul(2)
        .saturating_add(if first_insight_boost { 16 } else { 0 });
    if move_distance > 0 || strengthen_count > 0 {
        let to_index = card_index
            .saturating_add(usize::try_from(move_distance).ok()?)
            .min(hand_len.saturating_sub(1));
        if to_index != card_index {
            ops.push(super::BuffActRuleOp::subscriber_from_owner(
                RuleOp::Command(BattleCommand::Card(CardCommand::MoveServer {
                    origin,
                    from_index: card_index,
                    to_index,
                })),
            ));
            card_index = to_index;
        }
        let data = existing
            .and_then(|index| hand.get(index))
            .and_then(crate::engine::manager::card::unnamed::UnnamedCardData::from_card)
            .unwrap_or_default();
        for track in balanced_tracks(data, strengthen_count) {
            ops.push(super::BuffActRuleOp::subscriber_from_owner(
                RuleOp::Command(BattleCommand::Card(CardCommand::UpdateUnnamed(
                    CardUpdateUnnamed {
                        origin,
                        index: card_index,
                        lock: None,
                        strengthen_track: Some(track),
                        strengthen_amount: 1,
                    },
                ))),
            ));
        }
    }

    if card_index == hand_len.saturating_sub(1) {
        let locked = existing
            .and_then(|index| hand.get(index))
            .map(crate::engine::manager::card::unnamed::is_locked)
            .unwrap_or(true);
        if locked {
            ops.push(super::BuffActRuleOp::subscriber_from_owner(
                RuleOp::Command(BattleCommand::Card(CardCommand::UpdateUnnamed(
                    CardUpdateUnnamed {
                        origin,
                        index: card_index,
                        lock: Some(false),
                        strengthen_track: None,
                        strengthen_amount: 0,
                    },
                ))),
            ));
        }
    }

    Some(ops)
}

pub fn charging_rule_ops(
    managers: &BattleManagers,
    catalog: &SkillEffectCatalog,
    subscriber: &BuffActSubscriber,
    event: &BattleEvent,
) -> Option<Vec<RuleOp>> {
    if !super::subscriber_is_kind(subscriber, BuffActKind::UnnamedChargingSpCard) {
        return None;
    }
    let BattleEvent::SkillAction(action) = event else {
        return None;
    };
    if action.phase != SkillPhase::AfterHit
        || !matches!(
            action.mode,
            SkillExecutionMode::Active | SkillExecutionMode::DirectBig
        )
        || managers.entity.team_type(action.source_uid) != Some(subscriber.team_type)
    {
        return Some(Vec::new());
    }
    let track = enhancement_track(action.effect_tag, catalog.logic_target(action.skill_id))?;
    let index = managers
        .card
        .hand()
        .iter()
        .position(crate::engine::manager::card::unnamed::is_unnamed)?;
    Some(vec![RuleOp::Command(BattleCommand::Card(
        CardCommand::UpdateUnnamed(CardUpdateUnnamed {
            origin: super::command_origin(subscriber)?,
            index,
            lock: None,
            strengthen_track: Some(track),
            strengthen_amount: 1,
        }),
    ))])
}

pub fn move_and_strengthen_ops(
    managers: &BattleManagers,
    origin: crate::engine::skill::rule::CommandOrigin,
    distance: i32,
) -> Vec<RuleOp> {
    if distance <= 0 {
        return Vec::new();
    }
    let hand = managers.card.hand();
    let Some(from_index) = hand
        .iter()
        .position(crate::engine::manager::card::unnamed::is_unnamed)
    else {
        return Vec::new();
    };
    let to_index = from_index
        .saturating_add(usize::try_from(distance).unwrap_or_default())
        .min(hand.len().saturating_sub(1));
    let mut ops = Vec::new();
    if from_index != to_index {
        ops.push(RuleOp::Command(BattleCommand::Card(
            CardCommand::MoveServer {
                origin,
                from_index,
                to_index,
            },
        )));
    }
    let data = hand
        .get(from_index)
        .and_then(crate::engine::manager::card::unnamed::UnnamedCardData::from_card)
        .unwrap_or_default();
    ops.extend(
        balanced_tracks(data, distance.saturating_mul(2))
            .into_iter()
            .map(|track| {
                RuleOp::Command(BattleCommand::Card(CardCommand::UpdateUnnamed(
                    CardUpdateUnnamed {
                        origin,
                        index: to_index,
                        lock: None,
                        strengthen_track: Some(track),
                        strengthen_amount: 1,
                    },
                )))
            }),
    );
    ops
}

fn enhancement_track(effect_tag: i32, logic_target: i32) -> Option<i32> {
    match effect_tag {
        tag if matches!(tag, 1 | 2 | 3) => Some(if logic_target_is_multiple(logic_target) {
            2
        } else {
            1
        }),
        tag if matches!(tag, 4 | 5) => Some(3),
        tag if matches!(tag, 6 | 9) => Some(4),
        _ => None,
    }
}

fn logic_target_is_multiple(logic_target: i32) -> bool {
    matches!(
        logic_target,
        101 | 102
            | 104
            | 105
            | 117
            | 120
            | 121
            | 122
            | 123
            | 130
            | 132
            | 201
            | 202
            | 213
            | 214
            | 215
            | 219
            | 220
            | 232
            | 237
            | 238
            | 239
            | 240
            | 241
            | 249
            | 250
            | 301
            | 310
            | 312
            | 3000
            | 4000
    )
}

pub fn balanced_tracks(
    mut data: crate::engine::manager::card::unnamed::UnnamedCardData,
    count: i32,
) -> Vec<i32> {
    let mut output = Vec::new();
    for _ in 0..count.max(0) {
        let Some(track) = (1..=4)
            .filter(|track| {
                data.strengthen
                    .get(&track.to_string())
                    .copied()
                    .unwrap_or_default()
                    < crate::engine::manager::card::unnamed::UnnamedCardData::MAX_STRENGTHEN
            })
            .min_by_key(|track| {
                (
                    data.strengthen
                        .get(&track.to_string())
                        .copied()
                        .unwrap_or_default(),
                    *track,
                )
            })
        else {
            break;
        };
        data.strengthen(track, 1);
        output.push(track);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balanced_strengthening_prioritizes_the_fewest_layers() {
        let tracks = balanced_tracks(
            crate::engine::manager::card::unnamed::UnnamedCardData::default(),
            10,
        );
        assert_eq!(tracks, vec![1, 2, 3, 4, 1, 2, 3, 4, 1, 2]);
    }

    #[test]
    fn enhancement_type_matches_client_grouping() {
        assert_eq!(enhancement_track(2, 1), Some(1));
        assert_eq!(enhancement_track(3, 201), Some(2));
        assert_eq!(enhancement_track(4, 103), Some(3));
        assert_eq!(enhancement_track(6, 103), Some(4));
    }
}

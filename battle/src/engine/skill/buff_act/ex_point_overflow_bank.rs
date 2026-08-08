use crate::engine::{
    event::payload::BattleEvent,
    manager::{
        BattleManagers,
        buff::{BuffCommand, BuffSetState},
    },
    skill::{
        buff_act::BuffActRuleOp,
        rule::output::{BattleCommand, RuleOp},
        subscriber::BuffActSubscriber,
    },
};

pub fn rule_ops(
    managers: &BattleManagers,
    subscriber: &BuffActSubscriber,
    event: &BattleEvent,
) -> Option<Vec<BuffActRuleOp>> {
    let BattleEvent::ExPointOverflow(change) = event else {
        return Some(Vec::new());
    };
    if !matches!(
        subscriber.key.definition,
        crate::engine::skill::rule::DefinitionKey { opcode: 806, .. }
            | crate::engine::skill::rule::DefinitionKey { opcode: 1149, .. }
    ) || change.overflow <= 0
        || !change
            .kind
            .can_bank_overflow(change.kind, change.target_uid, subscriber.owner_uid)
    {
        return Some(Vec::new());
    }
    let capacity = subscriber.args.first().copied()? * subscriber.amount.max(1);
    let buff = managers
        .buff
        .snapshot(subscriber.owner_uid, subscriber.buff_uid)?;
    let current = current(&buff);
    let stored = stored_overflow(change.overflow, capacity, current);
    if stored <= 0 {
        return Some(Vec::new());
    }
    let effect_type = *super::wire::find(subscriber.key.definition.opcode, &subscriber.act_type)?
        .markers(super::wire::WirePhase::Refresh)
        .first()?;
    Some(vec![
        BuffActRuleOp::grouped_event(RuleOp::Command(BattleCommand::Buff(
            BuffCommand::SetInternalState(BuffSetState {
                ex_info: None,
                origin: super::command_origin(subscriber)?,
                target_uid: subscriber.owner_uid,
                buff_uid: subscriber.buff_uid,
                params: Some(format!(
                    "{}#{}",
                    subscriber.key.definition.opcode,
                    current + stored
                )),
                act_info: None,
            }),
        ))),
        BuffActRuleOp::grouped_event(RuleOp::BuffFeatureMarker {
            target_uid: subscriber.owner_uid,
            effect_type,
            effect_num: stored,
            buff_act_id: 0,
        }),
    ])
}

fn current(buff: &sonettobuf::BuffInfo) -> i32 {
    buff.act_common_params
        .as_deref()
        .and_then(|raw| raw.rsplit('#').next())
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or_default()
}

fn stored_overflow(overflow: i32, capacity: i32, current: i32) -> i32 {
    overflow.min((capacity - current).max(0))
}

#[cfg(test)]
mod tests {
    use sonettobuf::{BuffInfo, Fight, FightEntityInfo, FightTeam};

    use crate::engine::{
        event::{kind::EventKind, payload::ExPointChangeEvent, subscription::SubscriptionKey},
        manager::{BattleManagers, ex_point::ExPointKind},
        skill::rule::{CommandOrigin, DefinitionKey, RuleDomain},
    };

    use super::*;

    #[test]
    fn overflow_capacity_is_clamped() {
        assert_eq!(stored_overflow(2, 3, 0), 2);
        assert_eq!(stored_overflow(2, 3, 2), 1);
        assert_eq!(stored_overflow(1, 3, 3), 0);
    }

    #[test]
    fn overflow_emits_owned_state_and_value_marker_ops() {
        let fight = Fight {
            attacker: Some(FightTeam {
                entitys: vec![FightEntityInfo {
                    uid: Some(10),
                    current_hp: Some(100),
                    buffs: vec![BuffInfo {
                        uid: Some(20),
                        buff_id: Some(31250161),
                        act_common_params: Some("806#1".to_owned()),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let managers = BattleManagers::seeded(&fight);
        let subscriber = BuffActSubscriber {
            owner_uid: 10,
            source_uid: 10,
            buff_uid: 20,
            buff_id: 31250161,
            team_type: 1,
            owner_alive: true,
            amount: 1,
            key: SubscriptionKey::new(
                EventKind::ExPointOverflow,
                crate::engine::skill::rule::DefinitionKey::new(806, "ExPointOverflowBank"),
            ),
            act_type: "ExPointOverflowBank".to_owned(),
            effect_time: 18,
            effect_condition: 0,
            args: vec![3],
            raw: "806#3".to_owned(),
        };
        let event = BattleEvent::ExPointOverflow(ExPointChangeEvent {
            origin: CommandOrigin {
                domain: RuleDomain::Behavior,
                key: DefinitionKey::new(1, "AddExPoint"),
            },
            source_uid: 10,
            target_uid: 10,
            kind: ExPointKind::Common,
            before: 5,
            requested_delta: 2,
            applied_delta: 0,
            after: 5,
            overflow: 2,
        });

        let ops = rule_ops(&managers, &subscriber, &event).unwrap();

        assert!(ops.iter().all(|op| {
            op.scope == crate::engine::skill::buff_act::BuffActFrameScope::SubscriberFrame
                && op.frame_owner == crate::engine::skill::buff_act::BuffActFrameOwner::Event
                && op.group_with_siblings
        }));
        assert!(matches!(
            &ops[0].op,
            RuleOp::Command(BattleCommand::Buff(BuffCommand::SetInternalState(
                BuffSetState {
                    params: Some(params),
                    ..
                }
            ))) if params == "806#3"
        ));
        assert!(matches!(
            ops[1].op,
            RuleOp::BuffFeatureMarker {
                target_uid: 10,
                effect_type,
                effect_num: 2,
                ..
            } if effect_type
                == sonettobuf::effect_type_enum::EffectType::Expointoverflowbank as i32
        ));
    }
}

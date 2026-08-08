use sonettobuf::effect_type_enum::EffectType;

use crate::engine::{
    event::payload::BattleEvent,
    manager::{
        BattleManagers,
        buff::{BuffCommand, BuffConsume, BuffGrant, BuffSelector, DepletedBuff},
        eureka::{EUREKA_RESOURCE_ID, EurekaChange, EurekaCommand},
        gauge::{GaugeCommand, GaugeOperation},
    },
    mechanic::impromptu,
    skill::{
        rule::output::{BattleCommand, RuleOp},
        subscriber::BuffActSubscriber,
    },
};

pub fn supports_round_start(args: &[i32]) -> bool {
    matches!(args, [consume_buff, consume_eureka, buff_id, amount]
        if *consume_buff > 0 && *consume_eureka > 0 && *buff_id > 0 && *amount > 0)
}

pub fn supports_skill_cast(args: &[i32]) -> bool {
    matches!(args, [consume_eureka, inspiration] if *consume_eureka > 0 && *inspiration > 0)
}

pub fn round_start_rule_ops(
    managers: &BattleManagers,
    subscriber: &BuffActSubscriber,
    event: &BattleEvent,
) -> Option<Vec<RuleOp>> {
    if !matches!(event, BattleEvent::RoundStart) {
        return None;
    }
    let [consume_buff, consume_eureka, buff_id, amount] = subscriber.args.as_slice() else {
        return None;
    };
    if subscriber.amount < *consume_buff
        || managers
            .eureka
            .get(subscriber.owner_uid, EUREKA_RESOURCE_ID)
            .current
            < *consume_eureka
    {
        return Some(Vec::new());
    }
    let origin = super::command_origin(subscriber)?;
    Some(vec![
        RuleOp::Command(BattleCommand::Buff(BuffCommand::Consume(BuffConsume {
            origin,
            target_uid: subscriber.owner_uid,
            selector: BuffSelector::Uid(subscriber.buff_uid),
            amount: *consume_buff,
            depleted: DepletedBuff::Remove,
        }))),
        RuleOp::Command(BattleCommand::Eureka(EurekaCommand::Change(EurekaChange {
            origin,
            source_uid: subscriber.owner_uid,
            target_uid: subscriber.owner_uid,
            power_id: EUREKA_RESOURCE_ID,
            delta: -*consume_eureka,
            effect_type: EffectType::Powerchange as i32,
        }))),
        RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(BuffGrant {
            origin,
            source_uid: subscriber.owner_uid,
            target_uid: subscriber.owner_uid,
            buff_id: *buff_id,
            amount: Some(*amount),
            occurrences: 1,
            child_uid_reservations: 0,
        }))),
    ])
}

pub fn skill_cast_rule_ops(
    managers: &BattleManagers,
    subscriber: &BuffActSubscriber,
    event: &BattleEvent,
) -> Option<Vec<RuleOp>> {
    let BattleEvent::SkillAction(action) = event else {
        return None;
    };
    if action.source_uid != subscriber.owner_uid {
        return Some(Vec::new());
    }
    let [consume_eureka, inspiration] = subscriber.args.as_slice() else {
        return None;
    };
    let paper_heron_uid = subscriber.source_uid;
    if managers
        .eureka
        .get(paper_heron_uid, EUREKA_RESOURCE_ID)
        .current
        < *consume_eureka
        || managers
            .gauge
            .get(impromptu::inspiration_key(
                crate::engine::manager::emitter::UID,
            ))
            .is_none()
    {
        return Some(Vec::new());
    }
    let origin = super::command_origin(subscriber)?;
    Some(vec![
        RuleOp::Command(BattleCommand::Eureka(EurekaCommand::Change(EurekaChange {
            origin,
            source_uid: paper_heron_uid,
            target_uid: paper_heron_uid,
            power_id: EUREKA_RESOURCE_ID,
            delta: -*consume_eureka,
            effect_type: EffectType::Powerchange as i32,
        }))),
        RuleOp::Command(BattleCommand::Gauge(GaugeCommand::new(
            origin,
            impromptu::inspiration_key(crate::engine::manager::emitter::UID),
            GaugeOperation::ChangeValue {
                delta: *inspiration,
            },
        ))),
    ])
}

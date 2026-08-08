use crate::engine::{
    event::{kind::EventKind, payload::BattleEvent},
    manager::{
        BattleManagers,
        buff::{BuffCommand, BuffRemove, BuffRemoveSelector},
        gauge::{GaugeCommand, GaugeOperation},
    },
    mechanic::{bloodtithe, nuo_di_ka::NuoDiKaCommand},
    skill::{
        action::SkillRequest,
        rule::output::{BattleCommand, RuleOp},
        subscriber::BuffActSubscriber,
    },
};

pub fn supports(args: &[i32]) -> bool {
    matches!(args, [1, cost, max_groups, points_per_group, attack_skill, heal_skill]
        if *cost > 0
            && *max_groups > 0
            && *points_per_group > 0
            && *attack_skill > 0
            && *heal_skill > 0)
}

pub fn referenced_skills(args: &[i32]) -> impl Iterator<Item = i32> + '_ {
    args.get(4..6).into_iter().flatten().copied()
}

pub fn rule_ops(
    managers: &BattleManagers,
    catalog: &crate::engine::skill::effect::SkillEffectCatalog,
    subscriber: &BuffActSubscriber,
    event: &BattleEvent,
) -> Option<Vec<RuleOp>> {
    if event.kind() != EventKind::RoundEndAfterSettlement || !supports(&subscriber.args) {
        return None;
    }
    let [
        _,
        cost,
        max_groups,
        points_per_group,
        attack_skill,
        heal_skill,
    ] = subscriber.args.as_slice()
    else {
        return None;
    };
    let origin = super::command_origin(subscriber)?;
    let key = bloodtithe::rule::key(subscriber.team_type);
    let available = managers
        .gauge
        .get(key)
        .map(|state| state.current)
        .unwrap_or_default()
        .max(0);
    let groups = (available / *cost).min(*max_groups);
    if groups == 0 {
        return Some(vec![RuleOp::Skill(invocation(
            subscriber.owner_uid,
            *heal_skill,
            catalog,
        ))]);
    }

    let consumed = groups * *cost;
    let points = groups * *points_per_group;
    Some(vec![
        RuleOp::Command(BattleCommand::Gauge(
            GaugeCommand::new(
                origin,
                key,
                GaugeOperation::ChangeValue { delta: -consumed },
            )
            .attributed_to(subscriber.owner_uid, 0)
            .with_raw_delta(-consumed.saturating_mul(1000)),
        )),
        RuleOp::Command(BattleCommand::NuoDiKa(NuoDiKaCommand::Set {
            owner_uid: subscriber.owner_uid,
            points,
            bloodtithe_consumed: consumed,
            max_points: max_groups * points_per_group,
        })),
        RuleOp::Skill(invocation(subscriber.owner_uid, *attack_skill, catalog)),
        RuleOp::Command(BattleCommand::NuoDiKa(NuoDiKaCommand::Clear {
            owner_uid: subscriber.owner_uid,
        })),
        RuleOp::Command(BattleCommand::Buff(BuffCommand::Remove(BuffRemove {
            origin,
            target_uid: subscriber.owner_uid,
            selector: BuffRemoveSelector::Uid(subscriber.buff_uid),
        }))),
    ])
}

pub fn scoped_rule_ops(
    managers: &BattleManagers,
    catalog: &crate::engine::skill::effect::SkillEffectCatalog,
    subscriber: &BuffActSubscriber,
    event: &BattleEvent,
) -> Option<Vec<super::BuffActRuleOp>> {
    rule_ops(managers, catalog, subscriber, event).map(|ops| {
        ops.into_iter()
            .enumerate()
            .map(|(index, op)| match index {
                0..=2 => super::BuffActRuleOp::subscriber(op),
                3 => super::BuffActRuleOp::untargeted_event_from_owner(op),
                _ => super::BuffActRuleOp::separate_subscriber_from_owner(op),
            })
            .collect()
    })
}

fn invocation(
    owner_uid: i64,
    skill_id: i32,
    catalog: &crate::engine::skill::effect::SkillEffectCatalog,
) -> crate::engine::skill::action::SkillInvocation {
    let mut invocation: crate::engine::skill::action::SkillInvocation = SkillRequest {
        source_uid: owner_uid,
        skill_id,
    }
    .into();
    invocation.mode = crate::engine::skill::action::SkillExecutionMode::Active;
    invocation.extra_skill_kind = crate::engine::skill::condition::extra::skill_kind_from_is_extra(
        catalog.extra_kind(skill_id),
    );
    invocation
}

#[cfg(test)]
mod tests {
    use sonettobuf::{Fight, FightEntityInfo, FightTeam};

    use super::*;
    use crate::engine::{
        event::subscription::SubscriptionKey,
        skill::{
            buff_act::{
                BuffActFrameOwner, BuffActFrameScope, BuffActFrameSource, registry::BuffActKind,
            },
            rule::DefinitionKey,
        },
    };

    #[test]
    fn channel_spends_bloodtithe_stores_points_and_uses_configured_attack() {
        crate::test_support::init_config();
        let fight = Fight {
            attacker: Some(FightTeam {
                entitys: vec![FightEntityInfo {
                    uid: Some(10),
                    current_hp: Some(1),
                    team_type: Some(1),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut managers = BattleManagers::seeded(&fight);
        let origin =
            super::super::configured_command_origin(953, BuffActKind::BloodPoolTag).unwrap();
        managers
            .execute_gauge(GaugeCommand::new(
                origin,
                bloodtithe::rule::key(1),
                GaugeOperation::Enable { max: Some(56) },
            ))
            .unwrap();
        managers
            .execute_gauge(GaugeCommand::new(
                origin,
                bloodtithe::rule::key(1),
                GaugeOperation::ChangeValue { delta: 21 },
            ))
            .unwrap();

        let ops = rule_ops(
            &managers,
            &crate::engine::skill::effect::SkillEffectCatalog::default(),
            &subscriber(),
            &BattleEvent::Kind(EventKind::RoundEndAfterSettlement),
        )
        .unwrap();

        assert!(matches!(
            ops.as_slice(),
            [
                RuleOp::Command(BattleCommand::Gauge(GaugeCommand {
                    operation: GaugeOperation::ChangeValue { delta: -18 },
                    raw_delta: Some(-18_000),
                    ..
                })),
                RuleOp::Command(BattleCommand::NuoDiKa(NuoDiKaCommand::Set {
                    points: 9,
                    bloodtithe_consumed: 18,
                    max_points: 30,
                    ..
                })),
                RuleOp::Skill(invocation),
                RuleOp::Command(BattleCommand::NuoDiKa(NuoDiKaCommand::Clear {
                    owner_uid: 10,
                })),
                RuleOp::Command(BattleCommand::Buff(BuffCommand::Remove(BuffRemove {
                    target_uid: 10,
                    selector: BuffRemoveSelector::Uid(43),
                    ..
                }))),
            ] if invocation.plan.skill_id == 31200173
        ));
    }

    #[test]
    fn channel_uses_configured_heal_without_state_when_cost_is_unavailable() {
        crate::test_support::init_config();
        let mut managers = BattleManagers::default();
        let origin =
            super::super::configured_command_origin(953, BuffActKind::BloodPoolTag).unwrap();
        managers
            .execute_gauge(GaugeCommand::new(
                origin,
                bloodtithe::rule::key(1),
                GaugeOperation::Enable { max: Some(56) },
            ))
            .unwrap();

        let ops = rule_ops(
            &managers,
            &crate::engine::skill::effect::SkillEffectCatalog::default(),
            &subscriber(),
            &BattleEvent::Kind(EventKind::RoundEndAfterSettlement),
        )
        .unwrap();

        assert!(matches!(ops.as_slice(), [RuleOp::Skill(invocation)]
            if invocation.plan.skill_id == 31200182));
    }

    #[test]
    fn channel_uses_configured_heal_when_bloodtithe_gauge_is_absent() {
        let managers = BattleManagers::default();

        let ops = rule_ops(
            &managers,
            &crate::engine::skill::effect::SkillEffectCatalog::default(),
            &subscriber(),
            &BattleEvent::Kind(EventKind::RoundEndAfterSettlement),
        )
        .unwrap();

        assert!(matches!(ops.as_slice(), [RuleOp::Skill(invocation)]
            if invocation.plan.skill_id == 31200182));
    }

    #[test]
    fn channel_clear_is_an_owner_emitted_untargeted_event() {
        crate::test_support::init_config();
        let mut managers = BattleManagers::default();
        let origin =
            super::super::configured_command_origin(953, BuffActKind::BloodPoolTag).unwrap();
        managers
            .execute_gauge(GaugeCommand::new(
                origin,
                bloodtithe::rule::key(1),
                GaugeOperation::Enable { max: Some(56) },
            ))
            .unwrap();
        managers
            .execute_gauge(GaugeCommand::new(
                origin,
                bloodtithe::rule::key(1),
                GaugeOperation::ChangeValue { delta: 6 },
            ))
            .unwrap();

        let ops = scoped_rule_ops(
            &managers,
            &crate::engine::skill::effect::SkillEffectCatalog::default(),
            &subscriber(),
            &BattleEvent::Kind(EventKind::RoundEndAfterSettlement),
        )
        .unwrap();

        assert_eq!(ops[3].scope, BuffActFrameScope::ActionFrame);
        assert_eq!(ops[3].source, BuffActFrameSource::Owner);
        assert_eq!(ops[3].frame_owner, BuffActFrameOwner::UntargetedEvent);
        assert!(!ops[3].group_with_siblings);
    }

    fn subscriber() -> BuffActSubscriber {
        BuffActSubscriber {
            owner_uid: 10,
            source_uid: 10,
            buff_uid: 43,
            buff_id: 31200193,
            team_type: 1,
            owner_alive: true,
            amount: 1,
            key: SubscriptionKey::new(
                EventKind::RoundEndAfterSettlement,
                DefinitionKey::new(1006, "NuoDiKaCastChannel"),
            ),
            act_type: "NuoDiKaCastChannel".to_owned(),
            effect_time: 304,
            effect_condition: 0,
            args: vec![1, 6, 10, 3, 31200173, 31200182],
            raw: "1006#1#6#10#3#31200173#31200182".to_owned(),
        }
    }
}

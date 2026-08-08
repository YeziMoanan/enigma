use crate::engine::{
    event::payload::BattleEvent,
    manager::{
        BattleManagers,
        buff::{ActiveBuffFeature, BuffCommand, BuffGrant},
    },
    skill::{
        buff_act::registry::BuffActKind,
        rule::output::{BattleCommand, RuleOp},
    },
};

pub fn transaction_rule_ops(
    managers: &BattleManagers,
    event: &BattleEvent,
) -> Vec<(ActiveBuffFeature, RuleOp)> {
    let BattleEvent::ToughnessBroken { target_uid, .. } = event else {
        return Vec::new();
    };
    let Some(target_team) = managers.entity.team_type(*target_uid) else {
        return Vec::new();
    };

    managers
        .buff
        .active_features(&managers.hp)
        .into_iter()
        .filter(|feature| {
            super::is_kind(feature, BuffActKind::PerBrokenAddBuff)
                && managers
                    .entity
                    .team_type(feature.owner_uid)
                    .is_some_and(|team| team != target_team)
        })
        .filter_map(|feature| {
            let [_, buff_id, amount] = feature.values.as_slice() else {
                return None;
            };
            Some((
                feature.clone(),
                RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(BuffGrant {
                    origin: super::feature_command_origin(&feature)?,
                    source_uid: feature.owner_uid,
                    target_uid: feature.owner_uid,
                    buff_id: *buff_id,
                    amount: Some(*amount),
                    occurrences: 1,
                    child_uid_reservations: 0,
                }))),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use sonettobuf::{BuffInfo, Fight, FightEntityInfo, FightTeam};

    use super::*;

    fn managers() -> BattleManagers {
        crate::test_support::init_config();
        BattleManagers::seeded(&Fight {
            attacker: Some(FightTeam {
                entitys: vec![
                    FightEntityInfo {
                        uid: Some(10),
                        team_type: Some(1),
                        current_hp: Some(1_000),
                        buffs: vec![BuffInfo {
                            uid: Some(20),
                            buff_id: Some(31471009),
                            from_uid: Some(10),
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                    FightEntityInfo {
                        uid: Some(11),
                        team_type: Some(1),
                        current_hp: Some(1_000),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            defender: Some(FightTeam {
                entitys: vec![FightEntityInfo {
                    uid: Some(-1),
                    team_type: Some(2),
                    current_hp: Some(1_000),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    #[test]
    fn enemy_guard_break_grants_ms_stranger_configured_fragments() {
        let ops = transaction_rule_ops(
            &managers(),
            &BattleEvent::ToughnessBroken {
                source_uid: 10,
                target_uid: -1,
                skill_id: 31470111,
            },
        );

        assert!(matches!(
            ops.as_slice(),
            [(
                _,
                RuleOp::Command(BattleCommand::Buff(BuffCommand::Grant(grant)))
            )] if grant.source_uid == 10
                && grant.target_uid == 10
                && grant.buff_id == 31470001
                && grant.amount == Some(2)
        ));
    }

    #[test]
    fn allied_guard_break_does_not_grant_fragments() {
        assert!(
            transaction_rule_ops(
                &managers(),
                &BattleEvent::ToughnessBroken {
                    source_uid: -1,
                    target_uid: 11,
                    skill_id: 1,
                },
            )
            .is_empty()
        );
    }
}

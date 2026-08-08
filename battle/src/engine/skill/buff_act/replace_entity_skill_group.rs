use crate::engine::{
    event::payload::BattleEvent,
    manager::{
        BattleManagers,
        buff::{ActiveBuffFeature, BuffManager},
        card::{CardCommand, CardReplaceOwnerSkills},
    },
    skill::rule::output::{BattleCommand, RuleOp},
};

pub fn supports(args: &[i32]) -> bool {
    args.is_empty() || (args.len() >= 4 && args.iter().all(|value| *value > 0))
}

fn replacement_groups(raw: &str) -> Option<(Vec<i32>, Vec<i32>)> {
    let mut group1 = Vec::new();
    let mut group2 = Vec::new();
    for segment in raw.split('#').skip(1) {
        let (group, skills) = segment.split_once(':')?;
        let skills = skills
            .split(',')
            .filter_map(|value| value.trim().parse::<i32>().ok())
            .filter(|value| *value > 0)
            .collect::<Vec<_>>();
        match group.trim() {
            "1" => group1 = skills,
            "2" => group2 = skills,
            _ => return None,
        }
    }
    (!group1.is_empty() && !group2.is_empty()).then_some((group1, group2))
}

pub fn transaction_rule_ops(
    managers: &BattleManagers,
    event: &BattleEvent,
) -> Vec<(ActiveBuffFeature, RuleOp)> {
    let (change, added) = match event {
        BattleEvent::BuffAdded(change) => (change, true),
        BattleEvent::BuffRemoved(change) => (change, false),
        _ => return Vec::new(),
    };
    BuffManager::configured_features(change.buff_id)
        .into_iter()
        .filter_map(|mut feature| {
            if super::feature_kind(&feature)?
                != super::registry::BuffActKind::ReplaceEntitySkillGroup
            {
                return None;
            }
            let (replacement_group1, replacement_group2) = replacement_groups(&feature.raw)?;
            let entity = managers.entity_snapshot(change.target_uid)?;
            feature.owner_uid = change.target_uid;
            feature.source_uid = change.source_uid;
            feature.buff_uid = change.buff_uid;
            feature.amount = change.after_amount;
            let (base_group1, base_group2, replacement_group1, replacement_group2) = if added {
                (
                    entity.skill_group1,
                    entity.skill_group2,
                    replacement_group1,
                    replacement_group2,
                )
            } else {
                (
                    replacement_group1,
                    replacement_group2,
                    entity.skill_group1,
                    entity.skill_group2,
                )
            };
            Some((
                feature.clone(),
                RuleOp::Command(BattleCommand::Card(CardCommand::ReplaceOwnerSkills(
                    CardReplaceOwnerSkills {
                        origin: super::feature_command_origin(&feature)?,
                        owner_uid: change.target_uid,
                        base_group1,
                        base_group2,
                        replacement_group1,
                        replacement_group2,
                    },
                ))),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::event::payload::BuffChangeEvent;
    use sonettobuf::{Fight, FightEntityInfo, FightTeam};

    #[test]
    fn parses_both_colon_delimited_replacement_groups() {
        assert_eq!(
            replacement_groups("1138#1:11,12,13#2:21,22,23"),
            Some((vec![11, 12, 13], vec![21, 22, 23]))
        );
        assert_eq!(replacement_groups("1138#1:11,12,13"), None);
    }

    #[test]
    fn accepts_the_flattened_scanner_argument_shape() {
        assert!(supports(&[1, 11, 12, 13, 2, 21, 22, 23]));
        assert!(supports(&[]));
        assert!(!supports(&[1, 11, 0, 13]));
    }

    #[test]
    fn rhiannon_buff_addition_replaces_both_basic_skill_groups() {
        crate::test_support::init_config();
        let managers = BattleManagers::seeded(&Fight {
            attacker: Some(FightTeam {
                entitys: vec![FightEntityInfo {
                    uid: Some(10),
                    current_hp: Some(100),
                    skill_group1: vec![31460117, 31460118, 31460119],
                    skill_group2: vec![31460191, 31460192, 31460193],
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        });
        let ops = transaction_rule_ops(
            &managers,
            &BattleEvent::BuffAdded(BuffChangeEvent {
                source_uid: 10,
                target_uid: 10,
                buff_uid: 20,
                buff_id: 31460140,
                before_amount: 0,
                after_amount: 1,
                act_id: 0,
                act_value: 0,
            }),
        );

        assert!(matches!(
            ops.as_slice(),
            [(
                _,
                RuleOp::Command(BattleCommand::Card(CardCommand::ReplaceOwnerSkills(
                    CardReplaceOwnerSkills {
                        owner_uid: 10,
                        base_group1,
                        base_group2,
                        replacement_group1,
                        replacement_group2,
                        ..
                    }
                )))
            )] if base_group1 == &[31460117, 31460118, 31460119]
                && base_group2 == &[31460191, 31460192, 31460193]
                && replacement_group1 == &[31460241, 31460242, 31460243]
                && replacement_group2 == &[31460233, 31460234, 31460235]
        ));
    }
}

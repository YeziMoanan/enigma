use crate::engine::{
    entity::attr::AttrId,
    manager::{
        BattleManagers,
        buff::ActiveBuffFeature,
        hp::{HpCommand, MaxHpAdjust},
    },
    skill::{
        buff_act::{self, registry::BuffActKind},
        rule::output::{BattleCommand, RuleOp},
    },
};

pub fn supports(args: &[i32]) -> bool {
    matches!(args, [raw_attr, flat, permille]
        if AttrId::from_raw(*raw_attr).is_some() && *flat >= 0 && *permille != 0)
}

fn values(feature: &ActiveBuffFeature) -> Option<(AttrId, i32, i32)> {
    if !buff_act::is_kind(feature, BuffActKind::EachChangeAttrOneWay) {
        return None;
    }
    let [_, raw_attr, flat, permille] = feature.values.as_slice() else {
        return None;
    };
    Some((AttrId::from_raw(*raw_attr)?, *flat, *permille))
}

pub fn rate_delta(feature: &ActiveBuffFeature, attr_id: AttrId) -> i32 {
    values(feature)
        .filter(|(actual, _, _)| *actual == attr_id && attr_id != AttrId::Hp)
        .map(|(_, _, permille)| permille.saturating_mul(feature.amount))
        .unwrap_or_default()
}

pub fn flat_delta(feature: &ActiveBuffFeature, attr_id: AttrId) -> i32 {
    values(feature)
        .filter(|(actual, _, _)| *actual == attr_id && attr_id != AttrId::Hp)
        .map(|(_, flat, _)| flat.saturating_mul(feature.amount))
        .unwrap_or_default()
}

pub fn max_hp_rule_op(
    managers: &BattleManagers,
    feature: &ActiveBuffFeature,
    amount_delta: i32,
) -> Option<RuleOp> {
    let (attr_id, flat, permille) = values(feature)?;
    if attr_id != AttrId::Hp || amount_delta == 0 {
        return None;
    }
    let after_amount = feature.amount;
    let before_amount = after_amount.saturating_sub(amount_delta);
    let current_max = i128::from(managers.hp.max(feature.owner_uid));
    let before_rate = i128::from(1000_i32.saturating_add(permille.saturating_mul(before_amount)));
    let base_max = if before_rate == 0 {
        current_max
    } else {
        (current_max - i128::from(flat.saturating_mul(before_amount))) * 1000 / before_rate
    };
    let delta = ((base_max * i128::from(permille) / 1000 + i128::from(flat))
        * i128::from(amount_delta))
    .clamp(i128::from(i32::MIN), i128::from(i32::MAX)) as i32;
    (delta != 0).then_some(RuleOp::Command(BattleCommand::Hp(HpCommand::AdjustMax(
        MaxHpAdjust {
            origin: buff_act::feature_command_origin(feature)?,
            source_uid: if feature.source_uid != 0 {
                feature.source_uid
            } else {
                feature.owner_uid
            },
            target_uid: feature.owner_uid,
            delta,
        },
    ))))
}

pub fn transaction_rule_ops(
    managers: &BattleManagers,
    event: &crate::engine::event::payload::BattleEvent,
) -> Vec<(ActiveBuffFeature, RuleOp)> {
    super::attribute_transaction_rule_ops(
        managers,
        event,
        BuffActKind::EachChangeAttrOneWay,
        max_hp_rule_op,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonettobuf::{BuffInfo, Fight, FightEntityInfo, FightTeam, HeroAttribute};

    fn feature(attr_id: AttrId) -> ActiveBuffFeature {
        ActiveBuffFeature {
            owner_uid: 1,
            source_uid: 1,
            buff_uid: 2,
            buff_id: 3,
            amount: 1,
            team_type: 1,
            owner_alive: true,
            act_type: "EachChangeAttrOneWay".to_owned(),
            effect_time: 0,
            effect_condition: 0,
            raw: String::new(),
            values: vec![1131, attr_id.id(), 100, 310],
        }
    }

    #[test]
    fn exposes_the_configured_flat_and_rate_attack_lanes() {
        let feature = feature(AttrId::Attack);
        assert_eq!(rate_delta(&feature, AttrId::Attack), 310);
        assert_eq!(flat_delta(&feature, AttrId::Attack), 100);
        assert_eq!(rate_delta(&feature, AttrId::Hp), 0);
    }

    #[test]
    fn accepts_only_the_proven_three_argument_shape() {
        assert!(supports(&[AttrId::Attack.id(), 100, 310]));
        assert!(!supports(&[AttrId::Attack.id(), 100]));
        assert!(!supports(&[9999, 100, 310]));
    }

    #[test]
    fn rhiannon_attack_bonus_includes_the_flat_and_percentage_components() {
        crate::test_support::init_config();
        let managers = BattleManagers::seeded(&Fight {
            attacker: Some(FightTeam {
                entitys: vec![FightEntityInfo {
                    uid: Some(1),
                    current_hp: Some(1_000),
                    attr: Some(HeroAttribute {
                        hp: Some(1_000),
                        attack: Some(1_000),
                        ..Default::default()
                    }),
                    buffs: vec![BuffInfo {
                        uid: Some(2),
                        buff_id: Some(31460139),
                        from_uid: Some(1),
                        count: Some(1),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        });

        assert_eq!(managers.origin_attribute(1, AttrId::Attack), 1_410);
    }

    #[test]
    fn max_hp_addition_and_removal_are_exact_inverses() {
        crate::test_support::init_config();
        let mut managers = BattleManagers::seeded(&Fight {
            attacker: Some(FightTeam {
                entitys: vec![FightEntityInfo {
                    uid: Some(1),
                    current_hp: Some(1_000),
                    attr: Some(HeroAttribute {
                        hp: Some(1_000),
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        });
        let mut feature = feature(AttrId::Hp);
        feature.values = vec![1131, AttrId::Hp.id(), 100, 1550];
        let RuleOp::Command(BattleCommand::Hp(HpCommand::AdjustMax(add))) =
            max_hp_rule_op(&managers, &feature, 1).unwrap()
        else {
            panic!("expected max-HP adjustment")
        };
        assert_eq!(add.delta, 1_650);
        managers.hp.add_max_snapshot(1, add.delta);

        feature.amount = 0;
        let RuleOp::Command(BattleCommand::Hp(HpCommand::AdjustMax(remove))) =
            max_hp_rule_op(&managers, &feature, -1).unwrap()
        else {
            panic!("expected max-HP removal")
        };
        assert_eq!(remove.delta, -1_650);
    }
}

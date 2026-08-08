use super::*;

#[test]
fn ultimate_might_uses_the_skill_to_effect_alias() {
    init_config();
    let fight = Fight {
        attacker: Some(FightTeam {
            entitys: vec![entity(10, 1, 1, 1_000, 0)],
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![entity(-1, 2, 1, 0, 0)],
            ..Default::default()
        }),
        ..Default::default()
    };
    let pool = TargetPool::from_fight(&fight);
    let managers = BattleManagers::seeded(&fight);
    let command = resolve_attack_command(
        &AttackPlan {
            source_uid: 10,
            target_uid: -1,
            skill_id: 30230134,
            rate: 1_000,
            rate_terms: Vec::new(),
            attack_attributes: vec![(AttrId::UltimateMight, 180)],
            career_ratio_bonus: 0,
            attack_career: None,
            is_conduit: false,
            is_crit: false,
            assassinate: false,
            main_target: true,
            extra_skill_kind: 0,
            additional_enabled: false,
            additional_is_crit: None,
        },
        DamageRuntime {
            fight_version: 7,
            pool: &pool,
            attributes: &managers.attribute,
            buffs: &managers.buff,
            target_buffs: &managers.buff,
            hp: &managers.hp,
            fields: None,
            emitter: None,
            team_inspiration: 0,
        },
        CommandOrigin {
            domain: crate::engine::skill::rule::RuleDomain::Skill,
            key: crate::engine::skill::rule::DefinitionKey::new(30230134, "SkillDamage"),
        },
    )
    .unwrap();

    assert!(matches!(
        command,
        HpCommand::Damage(HpDamage { amount: 1_180, .. })
    ));
}

#[test]
fn ms_stranger_might_multipliers_apply_in_both_directions() {
    init_config();
    let fight = Fight {
        attacker: Some(FightTeam {
            entitys: vec![entity(10, 1, 1, 1_000, 0)],
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![entity(-1, 2, 1, 0, 0)],
            ..Default::default()
        }),
        ..Default::default()
    };
    let pool = TargetPool::from_fight(&fight);
    let managers = BattleManagers::seeded(&fight);
    let origin = CommandOrigin {
        domain: crate::engine::skill::rule::RuleDomain::Behavior,
        key: crate::engine::skill::rule::DefinitionKey::new(10004, "AttrFix"),
    };
    let shared_attributes = vec![
        (AttrId::UltimateMight, 100),
        (AttrId::IncantationMight, 200),
        (AttrId::UltimateMightMultiplier, 200),
        (AttrId::IncantationMightMultiplier, 200),
        (AttrId::IncantationSkillUltMightMultiplier, 1_000),
        (AttrId::UltimateIncantationMightMultiplier, 1_000),
    ];

    for (skill_id, expected) in [(1, 1_340), (30230134, 1_320)] {
        let command = resolve_attack_command(
            &AttackPlan {
                source_uid: 10,
                target_uid: -1,
                skill_id,
                rate: 1_000,
                rate_terms: Vec::new(),
                attack_attributes: shared_attributes.clone(),
                career_ratio_bonus: 0,
                attack_career: None,
                is_conduit: false,
                is_crit: false,
                assassinate: false,
                main_target: true,
                extra_skill_kind: 0,
                additional_enabled: false,
                additional_is_crit: None,
            },
            DamageRuntime {
                fight_version: 7,
                pool: &pool,
                attributes: &managers.attribute,
                buffs: &managers.buff,
                target_buffs: &managers.buff,
                hp: &managers.hp,
                fields: None,
                emitter: None,
                team_inspiration: 0,
            },
            origin,
        )
        .unwrap();

        assert!(matches!(
            command,
            HpCommand::Damage(HpDamage { amount, .. }) if amount == expected
        ));
    }
}

#[test]
fn playmode_attack_attributes_use_the_final_damage_lane() {
    init_config();
    let fight = Fight {
        attacker: Some(FightTeam {
            entitys: vec![entity(10, 1, 1, 1_000, 100)],
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![entity(-1, 2, 1, 0, 100)],
            ..Default::default()
        }),
        ..Default::default()
    };
    let pool = TargetPool::from_fight(&fight);
    let managers = BattleManagers::seeded(&fight);
    let origin = CommandOrigin {
        domain: crate::engine::skill::rule::RuleDomain::Behavior,
        key: crate::engine::skill::rule::DefinitionKey::new(10004, "AttrFix"),
    };

    for attack_attributes in [
        vec![(AttrId::PlaymodeDmgIncrease, -150)],
        vec![(AttrId::PlaymodeDmgImmunity, 150)],
    ] {
        let command = resolve_attack_command(
            &AttackPlan {
                source_uid: 10,
                target_uid: -1,
                skill_id: 1,
                rate: 1_000,
                rate_terms: Vec::new(),
                attack_attributes,
                career_ratio_bonus: 0,
                attack_career: None,
                is_conduit: false,
                is_crit: false,
                assassinate: false,
                main_target: true,
                extra_skill_kind: 0,
                additional_enabled: false,
                additional_is_crit: None,
            },
            DamageRuntime {
                fight_version: 6,
                pool: &pool,
                attributes: &managers.attribute,
                buffs: &managers.buff,
                target_buffs: &managers.buff,
                hp: &managers.hp,
                fields: None,
                emitter: None,
                team_inspiration: 0,
            },
            origin,
        )
        .unwrap();

        assert!(matches!(
            command,
            HpCommand::Damage(HpDamage { amount: 765, .. })
        ));
    }
}

#[test]
fn conduit_might_only_scales_conduit_skills() {
    init_config();
    let fight = Fight {
        attacker: Some(FightTeam {
            entitys: vec![entity(10, 1, 1, 1_000, 0)],
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![entity(-1, 2, 1, 0, 0)],
            ..Default::default()
        }),
        ..Default::default()
    };
    let pool = TargetPool::from_fight(&fight);
    let managers = BattleManagers::seeded(&fight);
    let origin = CommandOrigin {
        domain: crate::engine::skill::rule::RuleDomain::Behavior,
        key: crate::engine::skill::rule::DefinitionKey::new(10004, "AttrFix"),
    };

    for (is_conduit, expected) in [(false, 1_000), (true, 1_300)] {
        let command = resolve_attack_command(
            &AttackPlan {
                source_uid: 10,
                target_uid: -1,
                skill_id: 1,
                rate: 1_000,
                rate_terms: Vec::new(),
                attack_attributes: vec![(AttrId::ConduitMight, 300)],
                career_ratio_bonus: 0,
                attack_career: None,
                is_conduit,
                is_crit: false,
                assassinate: false,
                main_target: true,
                extra_skill_kind: 0,
                additional_enabled: false,
                additional_is_crit: None,
            },
            DamageRuntime {
                fight_version: 7,
                pool: &pool,
                attributes: &managers.attribute,
                buffs: &managers.buff,
                target_buffs: &managers.buff,
                hp: &managers.hp,
                fields: None,
                emitter: None,
                team_inspiration: 0,
            },
            origin,
        )
        .unwrap();

        assert!(matches!(
            command,
            HpCommand::Damage(HpDamage { amount, .. }) if amount == expected
        ));
    }
}

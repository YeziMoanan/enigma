use super::*;

#[test]
fn team_injury_change_projects_each_committed_counter_value() {
    let effects = project_change_for_test(&BattleChange::Injury(InjuryChange {
        origin: CommandOrigin {
            domain: RuleDomain::Skill,
            key: DefinitionKey::new(1, "TeamInjury"),
        },
        source_uid: 10,
        team_type: 1,
        counter_owner_uid: 11,
        before: 2,
        after: 4,
    }))
    .unwrap();

    assert_eq!(effects.len(), 2);
    assert!(effects.iter().all(|effect| {
        effect.target_id == Some(11)
            && effect.effect_type == Some(EffectType::Fightcounter as i32)
            && effect.effect_num
                == Some(crate::engine::manager::injury::InjuryCounterKind::TeamInjury.id())
            && effect.team_type == Some(1)
    }));
    assert_eq!(
        effects
            .iter()
            .map(|effect| effect.config_effect)
            .collect::<Vec<_>>(),
        vec![Some(3), Some(4)]
    );
}

#[test]
fn zero_cost_conduit_activation_has_no_cost_projection() {
    for change in [
        crate::engine::manager::conduit::ConduitChange::SkillBegan {
            source_uid: 10,
            team: 1,
            skill_id: 31490151,
            power_id: 999,
            activation_cost: 0,
            spent: 0,
        },
        crate::engine::manager::conduit::ConduitChange::SkillCostCommitted {
            source_uid: 10,
            team: 1,
            skill_id: 31490151,
            activation_cost: 0,
            consumed_this_round: 0,
        },
    ] {
        assert!(
            project_change_for_test(&BattleChange::Conduit(change))
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn client_conduit_group_selection_projects_one_configless_confirmation() {
    let effects = project_change_for_test(&BattleChange::Conduit(
        crate::engine::manager::conduit::ConduitChange::GroupSelected {
            source_uid: 263_811_366,
            team: 1,
            group: 1,
        },
    ))
    .unwrap();

    assert_eq!(effects.len(), 1);
    let [effect] = effects.as_slice() else {
        panic!("expected one client conduit selection effect");
    };
    assert_eq!(effect.target_id, Some(263_811_366));
    assert_eq!(
        effect.effect_type,
        Some(EffectType::Deviceskillindex as i32)
    );
    assert_eq!(effect.effect_num, Some(1));
    assert_eq!(effect.team_type, Some(1));
    assert_eq!(effect.config_effect, Some(0));
}

use super::*;
use sonettobuf::{FightTeam, HeroAttribute};

#[test]
fn grouped_career_exposes_each_configured_afflatus() {
    crate::test_support::init_config();
    let entity = TargetEntity::from_fight_entity(&FightEntityInfo {
        uid: Some(1),
        current_hp: Some(1),
        career: Some(101),
        ..Default::default()
    })
    .unwrap();

    assert!(entity.has_career(1));
    assert!(entity.has_career(2));
    assert!(!entity.has_career(3));
}

#[test]
fn selected_destiny_stone_replaces_character_battle_tags() {
    crate::test_support::init_config();
    let entity = TargetEntity::from_fight_entity(&FightEntityInfo {
        uid: Some(1),
        model_id: Some(3081),
        current_hp: Some(1),
        destiny_stone: Some(308101),
        destiny_rank: Some(1),
        ..Default::default()
    })
    .unwrap();

    assert_eq!(entity.battle_tags, vec![102, 114, 116]);
}

#[test]
fn monster_hidden_stats_use_the_instance_job_curve() {
    crate::test_support::init_config();
    let fight = Fight {
        defender: Some(FightTeam {
            entitys: vec![FightEntityInfo {
                uid: Some(-1),
                model_id: Some(30110801),
                entity_type: Some(2),
                level: Some(170),
                current_hp: Some(100),
                attr: Some(HeroAttribute {
                    hp: Some(100),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    };

    let entity = TargetPool::from_fight(&fight).defender_main.remove(0);

    assert_eq!(entity.crit_rate, 48);
    assert_eq!(entity.crit_dmg, 1200);
    assert_eq!(entity.crit_def, 387);
    assert_eq!(entity.add_dmg, 140);
    assert_eq!(entity.drop_dmg, 318);
}

#[test]
fn skill_slot_resolves_card_skill_ids_to_their_configured_effect() {
    crate::test_support::init_config();
    let pool = TargetPool::from_fight(&Fight {
        attacker: Some(FightTeam {
            entitys: vec![FightEntityInfo {
                uid: Some(1),
                current_hp: Some(100),
                attr: Some(HeroAttribute {
                    hp: Some(100),
                    ..Default::default()
                }),
                skill_group1: vec![31260111, 31260112, 31260113],
                skill_group2: vec![31260121, 31260122, 31260123],
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    });
    let managers = BattleManagers::default();

    assert_eq!(pool.skill_slot(&managers, 1, 31260171), 1);
    assert_eq!(pool.skill_slot(&managers, 1, 31260172), 1);
    assert_eq!(pool.skill_slot(&managers, 1, 31260121), 2);
}

#[test]
fn emitter_uses_average_attacker_stats_without_joining_the_team() {
    let entity = |uid, attack| FightEntityInfo {
        uid: Some(uid),
        current_hp: Some(100),
        attr: Some(HeroAttribute {
            hp: Some(100),
            attack: Some(attack),
            ..Default::default()
        }),
        ..Default::default()
    };
    let pool = TargetPool::from_fight(&Fight {
        attacker: Some(FightTeam {
            entitys: vec![entity(1, 100), entity(2, 300)],
            player_entity: Some(entity(0, 0)),
            ..Default::default()
        }),
        defender: Some(FightTeam {
            entitys: vec![entity(-1, 200)],
            ..Default::default()
        }),
        ..Default::default()
    });

    assert_eq!(
        pool.entity(crate::engine::manager::emitter::UID)
            .unwrap()
            .attack,
        200
    );
    assert_eq!(pool.attacker_main.len(), 2);
    assert_eq!(pool.team_type(0), Some(1));
    assert_eq!(
        pool.team_type(crate::engine::manager::emitter::UID),
        Some(1)
    );
    assert_eq!(
        pool.enemies(crate::engine::manager::emitter::UID, false)
            .iter()
            .map(|entity| entity.uid)
            .collect::<Vec<_>>(),
        vec![-1]
    );
    assert!(pool.enemies(404, false).is_empty());
}

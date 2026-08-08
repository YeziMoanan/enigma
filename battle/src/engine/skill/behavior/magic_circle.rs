use crate::engine::{
    entity::attr::AttrId,
    manager::{
        BattleManagers,
        buff::{BuffCommand, BuffGrantChild, BuffRemove, BuffRemoveSelector},
        field::{FieldCommand, FieldDefinition, FieldOperation, FieldThreshold},
    },
    runtime::determinism::RoundDeterminism,
    skill::{
        behavior::{BehaviorOpContext, classify::BehaviorKind, registry::BehaviorHandler},
        condition::conditions_match,
        effect::{ParsedBehavior, SkillEffectCatalog},
        rule::{
            RuleReferences,
            output::{BattleCommand, RuleOp},
        },
        target::{TargetContext, TargetPool, TargetResolver},
    },
};

pub fn deploy_rule_ops(
    behavior: &ParsedBehavior,
    source_uid: i64,
    team: i32,
    managers: &BattleManagers,
    pool: &TargetPool,
) -> Option<Vec<RuleOp>> {
    if behavior.spec.kind != BehaviorKind::AddMagicCircle {
        return None;
    }
    let circle_id = behavior.arg(0)?;
    let row = config::try_get()?.magic_circle.get(circle_id)?;
    let origin = super::command_origin(behavior)?;
    let current = managers.field.get(team);
    if current.is_some_and(|field| field.definition.field_id == circle_id) {
        return Some(Vec::new());
    }
    let initial_level = behavior.arg(1).unwrap_or_else(|| {
        (row.circle_type == 2)
            .then_some(circle_id % 10)
            .unwrap_or_default()
    });
    let operation = if current.is_some() {
        FieldOperation::Replace {
            definition: FieldDefinition {
                field_id: circle_id,
                duration: row.round,
            },
            create_uid: source_uid,
            level: initial_level,
        }
    } else {
        FieldOperation::DeployIfAbsent {
            definition: FieldDefinition {
                field_id: circle_id,
                duration: row.round,
            },
            create_uid: source_uid,
            initial_level,
            thresholds: field_thresholds(circle_id, team, managers),
        }
    };
    let field = RuleOp::Command(BattleCommand::Field(FieldCommand {
        origin,
        team,
        operation,
    }));
    let (ally_buffs, enemy_buffs) = crate::engine::mechanic::magic_circle::linked_buffs(circle_id);
    let grants = pool
        .allies(source_uid)
        .iter()
        .flat_map(|entity| ally_buffs.iter().map(move |buff_id| (entity.uid, *buff_id)))
        .chain(pool.enemies(source_uid, true).iter().flat_map(|entity| {
            enemy_buffs
                .iter()
                .map(move |buff_id| (entity.uid, *buff_id))
        }));
    let mut ops = current
        .into_iter()
        .flat_map(|field| {
            let (old_ally_buffs, old_enemy_buffs) =
                crate::engine::mechanic::magic_circle::linked_buffs(field.definition.field_id);
            pool.allies(source_uid)
                .iter()
                .flat_map(|entity| {
                    old_ally_buffs
                        .iter()
                        .map(move |buff_id| (entity.uid, *buff_id))
                })
                .chain(pool.enemies(source_uid, true).iter().flat_map(|entity| {
                    old_enemy_buffs
                        .iter()
                        .map(move |buff_id| (entity.uid, *buff_id))
                }))
                .map(|(target_uid, buff_id)| {
                    RuleOp::Command(BattleCommand::Buff(BuffCommand::Remove(BuffRemove {
                        origin,
                        target_uid,
                        selector: BuffRemoveSelector::ExactId(buff_id),
                    })))
                })
                .collect::<Vec<_>>()
        })
        .chain(grants.map(|(target_uid, buff_id)| {
            RuleOp::Command(BattleCommand::Buff(BuffCommand::GrantChild(
                BuffGrantChild {
                    origin,
                    source_uid,
                    target_uid,
                    buff_id,
                    amount: None,
                    params: None,
                    act_info: None,
                },
            )))
        }))
        .collect::<Vec<_>>();
    ops.push(field);
    Some(ops)
}

fn update_wang_qi_rule_ops(
    behavior: &ParsedBehavior,
    source_uid: i64,
    team: i32,
    managers: &BattleManagers,
    pool: &TargetPool,
) -> Option<Vec<RuleOp>> {
    let delta = behavior.arg(0)?;
    let current = managers.field.get(team)?;
    let family = current.definition.field_id / 10;
    let current_level = current.definition.field_id % 10;
    let next_level = current_level.saturating_add(delta).clamp(1, 4);
    let next_id = family.saturating_mul(10).saturating_add(next_level);
    if next_id == current.definition.field_id {
        return Some(Vec::new());
    }
    let replacement = ParsedBehavior::from_spec(
        crate::engine::skill::behavior::classify::BehaviorSpec::new(50019, "AddMagicCircle"),
        vec![next_id, next_level],
        Vec::new(),
    );
    deploy_rule_ops(&replacement, source_uid, team, managers, pool)
}

pub(crate) fn field_thresholds(
    circle_id: i32,
    team: i32,
    managers: &BattleManagers,
) -> Vec<FieldThreshold> {
    let Some(db) = config::try_get() else {
        return Vec::new();
    };
    let mut thresholds = db
        .fight_dnsz
        .iter()
        .filter_map(|threshold| {
            let circle = db.magic_circle.get(threshold.id)?;
            Some(FieldThreshold {
                level: threshold.level,
                progress: threshold.progress,
                definition: FieldDefinition {
                    field_id: threshold.id,
                    duration: circle.round,
                },
            })
        })
        .collect::<Vec<_>>();
    thresholds.sort_by_key(|threshold| threshold.level);
    if !thresholds
        .iter()
        .any(|threshold| threshold.definition.field_id == circle_id)
    {
        return Vec::new();
    }
    crate::engine::skill::buff_act::fix_electric_upgrade::resolve_thresholds(
        team,
        &thresholds,
        &managers.buff.active_features(&managers.hp),
    )
}

pub(super) struct Handler;

fn runtime_rule_ops(
    behavior: &ParsedBehavior,
    source_uid: i64,
    source_team: i32,
    managers: &BattleManagers,
    pool: &TargetPool,
) -> Option<Vec<RuleOp>> {
    match behavior.spec.kind {
        BehaviorKind::AddMagicCircle => {
            deploy_rule_ops(behavior, source_uid, source_team, managers, pool)
        }
        BehaviorKind::UpdateWangQiMagicCircle => {
            update_wang_qi_rule_ops(behavior, source_uid, source_team, managers, pool)
        }
        BehaviorKind::MagicCircleAttr if Handler::supports(behavior) => Some(Vec::new()),
        _ => None,
    }
}

impl BehaviorHandler for Handler {
    const VALIDATES_ARGUMENTS: bool = true;

    fn supports(behavior: &ParsedBehavior) -> bool {
        match behavior.spec.kind {
            BehaviorKind::AddMagicCircle => {
                let [circle_id, rest @ ..] = behavior.args.as_slice() else {
                    return false;
                };
                rest.len() <= 1
                    && rest.first().is_none_or(|level| *level >= 0)
                    && config::try_get().is_some_and(|db| db.magic_circle.get(*circle_id).is_some())
            }
            BehaviorKind::UpdateWangQiMagicCircle => {
                matches!(behavior.args.as_slice(), [delta] if *delta > 0)
            }
            BehaviorKind::MagicCircleAttr => {
                !behavior.args.is_empty()
                    && behavior.args.len().is_multiple_of(3)
                    && behavior
                        .args
                        .chunks_exact(3)
                        .all(|args| matches!(args[0], 1 | 2) && AttrId::from_raw(args[1]).is_some())
            }
            _ => false,
        }
    }

    fn emit_ops(context: BehaviorOpContext<'_>, behavior: &ParsedBehavior) -> Option<Vec<RuleOp>> {
        runtime_rule_ops(
            behavior,
            context.source_uid,
            context.source_team,
            context.managers,
            context.pool,
        )
    }

    fn references(behavior: &ParsedBehavior) -> RuleReferences {
        references(behavior)
    }
}

fn references(behavior: &ParsedBehavior) -> RuleReferences {
    RuleReferences {
        skills: (behavior.spec.kind == BehaviorKind::AddMagicCircle)
            .then(|| behavior.arg(0))
            .flatten()
            .into_iter()
            .flat_map(self_skills)
            .collect(),
        buffs: Vec::new(),
        models: Vec::new(),
    }
}

pub fn self_skills(circle_id: i32) -> Vec<i32> {
    config::try_get()
        .and_then(|db| db.magic_circle.get(circle_id))
        .map(|row| {
            let mut skills = row
                .self_skills
                .split(['|', '#'])
                .filter_map(|id| id.trim().parse::<i32>().ok())
                .filter(|id| *id > 0)
                .collect::<Vec<_>>();
            skills.extend(
                row.complex_effect
                    .split('|')
                    .filter_map(|entry| entry.split_once(':').map(|(_, value)| value))
                    .filter_map(|value| value.split(',').nth(1))
                    .filter_map(|id| id.trim().parse::<i32>().ok())
                    .filter(|id| *id > 0),
            );
            skills.sort_unstable();
            skills.dedup();
            skills
        })
        .unwrap_or_default()
}

pub fn active_self_skills(
    source_uid: i64,
    managers: &BattleManagers,
    pool: &TargetPool,
) -> Vec<i32> {
    pool.team_type(source_uid)
        .and_then(|team| managers.field.get(team))
        .map(|field| self_skills(field.definition.field_id))
        .unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
pub fn emit_attack_attributes(
    attack_attributes: &mut Vec<(AttrId, i32)>,
    source_uid: i64,
    active_skill_id: i32,
    effects: &SkillEffectCatalog,
    managers: &BattleManagers,
    pool: &TargetPool,
    determinism: &mut RoundDeterminism,
    context: TargetContext,
) {
    let Some(field) = pool
        .team_type(source_uid)
        .and_then(|team| managers.field.get(team))
    else {
        return;
    };
    let Some(owner) = pool.entity(field.create_uid) else {
        return;
    };
    if pool.source_is_attacker(owner.uid) != pool.source_is_attacker(source_uid) {
        return;
    }

    for passive_skill in &owner.passive_skills {
        let Some(effect) = effects.get(*passive_skill) else {
            continue;
        };
        for slot in effect
            .slots
            .iter()
            .filter(|slot| slot.behavior.spec.kind == BehaviorKind::MagicCircleAttr)
        {
            let condition_targets = TargetResolver::resolve_with_managers_and_context(
                &slot.condition_target,
                active_skill_id,
                owner.uid,
                pool,
                determinism,
                Some(managers),
                context,
            );
            if !conditions_match(
                &slot.conditions,
                owner.uid,
                &condition_targets,
                Some(managers),
                pool,
                context,
            ) {
                continue;
            }
            for args in slot.behavior.args.chunks_exact(3) {
                let [scope, raw_attr_id, delta] = args else {
                    continue;
                };
                if *scope == 1 && source_uid != owner.uid {
                    continue;
                }
                if !matches!(*scope, 1 | 2) {
                    continue;
                }
                let Some(attr_id) = AttrId::from_raw(*raw_attr_id) else {
                    continue;
                };
                attack_attributes.push((attr_id, *delta));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_circle_exposes_its_start_phase_skill() {
        crate::test_support::init_config();

        assert_eq!(self_skills(100051), vec![308801821]);
    }

    #[test]
    fn add_magic_circle_emits_a_config_derived_deploy_command() {
        crate::test_support::init_config();
        let behavior = ParsedBehavior::new(50019, "AddMagicCircle", vec![30001, 1]);

        let fight = sonettobuf::Fight {
            attacker: Some(sonettobuf::FightTeam {
                entitys: vec![sonettobuf::FightEntityInfo {
                    uid: Some(10),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let pool = TargetPool::from_fight(&fight);

        assert!(matches!(
            deploy_rule_ops(&behavior, 10, 1, &BattleManagers::default(), &pool).as_deref(),
            Some([RuleOp::Command(BattleCommand::Field(FieldCommand {
                team: 1,
                operation: FieldOperation::DeployIfAbsent {
                    definition: FieldDefinition {
                        field_id: 30001,
                        ..
                    },
                    create_uid: 10,
                    initial_level: 1,
                    thresholds,
                },
                ..
            }))]) if thresholds.iter().any(|threshold| threshold.level == 2 && threshold.progress == 50)
        ));
    }

    #[test]
    fn blood_domain_deployment_grants_its_configured_ally_buff() {
        crate::test_support::init_config();
        let fight = sonettobuf::Fight {
            attacker: Some(sonettobuf::FightTeam {
                entitys: vec![
                    sonettobuf::FightEntityInfo {
                        uid: Some(10),
                        ..Default::default()
                    },
                    sonettobuf::FightEntityInfo {
                        uid: Some(11),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            ..Default::default()
        };
        let pool = TargetPool::from_fight(&fight);
        let behavior = ParsedBehavior::new(50019, "AddMagicCircle", vec![100051]);

        let ops = deploy_rule_ops(&behavior, 10, 1, &BattleManagers::default(), &pool).unwrap();

        assert_eq!(ops.len(), 3);
        assert!(ops[..2].iter().all(|op| matches!(
            op,
            RuleOp::Command(BattleCommand::Buff(BuffCommand::GrantChild(
                BuffGrantChild {
                    buff_id: 308801312,
                    ..
                }
            )))
        )));
        assert!(matches!(ops[2], RuleOp::Command(BattleCommand::Field(_))));
    }

    #[test]
    fn magic_circle_attributes_are_valid_passive_effects_without_runtime_commands() {
        crate::test_support::init_config();
        let behavior = ParsedBehavior::new(60076, "MagicCircleAttr", vec![2, 214, 60]);
        let fight = sonettobuf::Fight {
            attacker: Some(sonettobuf::FightTeam {
                entitys: vec![sonettobuf::FightEntityInfo {
                    uid: Some(10),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let pool = TargetPool::from_fight(&fight);

        assert!(Handler::supports(&behavior));
        assert_eq!(behavior.spec.kind, BehaviorKind::MagicCircleAttr);
        assert_eq!(
            runtime_rule_ops(&behavior, 10, 1, &BattleManagers::default(), &pool),
            Some(Vec::new())
        );
    }
}

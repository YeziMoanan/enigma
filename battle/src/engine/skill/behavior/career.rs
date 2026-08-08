use crate::engine::skill::{
    behavior::{
        AttackModifierContext, BehaviorOpContext, classify::BehaviorKind, registry::BehaviorHandler,
    },
    effect::ParsedBehavior,
    rule::output::RuleOp,
};

pub struct Handler;

impl BehaviorHandler for Handler {
    const VALIDATES_ARGUMENTS: bool = true;

    fn supports(behavior: &ParsedBehavior) -> bool {
        matches!(
            (behavior.spec.kind, behavior.args.as_slice()),
            (BehaviorKind::CareerRatioFix, [bonus]) if *bonus != 0
        ) || matches!(
            (behavior.spec.kind, behavior.args.as_slice()),
            (BehaviorKind::ChangeAttackCareer, [career]) if (1..=8).contains(career)
        ) || matches!(
            (behavior.spec.kind, behavior.args.as_slice()),
            (BehaviorKind::SetCareerRestraint, [])
        )
    }

    fn emit_ops(context: BehaviorOpContext<'_>, behavior: &ParsedBehavior) -> Option<Vec<RuleOp>> {
        if behavior.spec.kind == BehaviorKind::SetCareerRestraint && Self::supports(behavior) {
            return Some(Vec::new());
        }
        apply(
            context.modifiers,
            behavior,
            context.pool,
            context.target_uid,
        )
        .then(Vec::new)
    }

    fn collect_attack_modifier(
        context: AttackModifierContext<'_>,
        behavior: &ParsedBehavior,
    ) -> bool {
        let target_uid = if behavior.spec.kind == BehaviorKind::SetCareerRestraint {
            let target = &context.operation.target;
            if target.hit_target_uid != 0 {
                target.hit_target_uid
            } else if target.runtime_target_uid != 0 {
                target.runtime_target_uid
            } else {
                context.operation.target_uid
            }
        } else {
            context.operation.target_uid
        };
        apply(
            context.operation.modifiers,
            behavior,
            context.operation.pool,
            target_uid,
        )
    }
}

fn apply(
    modifiers: &mut crate::engine::skill::action::SkillModifiers,
    behavior: &ParsedBehavior,
    pool: &crate::engine::skill::target::TargetPool,
    target_uid: i64,
) -> bool {
    if !Handler::supports(behavior) {
        return false;
    }
    match behavior.spec.kind {
        BehaviorKind::CareerRatioFix => {
            modifiers.career_ratio_bonus += behavior.args[0];
        }
        BehaviorKind::ChangeAttackCareer => {
            modifiers.attack_career = Some(behavior.args[0]);
        }
        BehaviorKind::SetCareerRestraint => {
            let Some(target) = pool.entity(target_uid) else {
                return false;
            };
            modifiers.attack_career = target.weak_careers.first().copied().or_else(|| {
                (1..=8).find(|career| {
                    crate::engine::damage::handler::restrains_target(*career, target)
                })
            });
            if modifiers.attack_career.is_none() {
                return false;
            }
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn career_ratio_fix_keeps_signed_permille_values() {
        assert!(Handler::supports(&ParsedBehavior::new(
            60058,
            "CareerRatioFix",
            vec![300],
        )));
        assert!(Handler::supports(&ParsedBehavior::new(
            60058,
            "CareerRatioFix",
            vec![-600],
        )));
    }

    #[test]
    fn attack_career_accepts_only_real_afflatuses() {
        assert!(Handler::supports(&ParsedBehavior::new(
            100036,
            "SkillChangeAttackCareer",
            vec![1],
        )));
        assert!(!Handler::supports(&ParsedBehavior::new(
            100036,
            "SkillChangeAttackCareer",
            vec![101],
        )));
    }

    #[test]
    fn attack_career_is_written_to_the_skill_modifier() {
        let managers = crate::engine::manager::BattleManagers::default();
        let pool = crate::engine::skill::target::TargetPool::default();
        let mut determinism = crate::engine::runtime::determinism::RoundDeterminism::default();
        let mut modifiers = crate::engine::skill::action::SkillModifiers::default();
        let mut target = crate::engine::skill::target::TargetContext::default();

        assert!(Handler::collect_attack_modifier(
            AttackModifierContext {
                operation: BehaviorOpContext {
                    source_uid: 10,
                    source_team: 1,
                    target_uid: -1,
                    active_skill_id: 20,
                    transfer_count: 1,
                    event: None,
                    managers: &managers,
                    pool: &pool,
                    determinism: &mut determinism,
                    modifiers: &mut modifiers,
                    target: &mut target,
                },
                conditions: &[],
            },
            &ParsedBehavior::new(100036, "SkillChangeAttackCareer", vec![1]),
        ));
        assert_eq!(modifiers.attack_career, Some(1));
    }

    #[test]
    fn active_skill_attack_career_uses_the_same_modifier_path() {
        let managers = crate::engine::manager::BattleManagers::default();
        let pool = crate::engine::skill::target::TargetPool::default();
        let mut determinism = crate::engine::runtime::determinism::RoundDeterminism::default();
        let mut modifiers = crate::engine::skill::action::SkillModifiers::default();
        let mut target = crate::engine::skill::target::TargetContext::default();

        assert_eq!(
            Handler::emit_ops(
                BehaviorOpContext {
                    source_uid: 10,
                    source_team: 1,
                    target_uid: -1,
                    active_skill_id: 20,
                    transfer_count: 1,
                    event: None,
                    managers: &managers,
                    pool: &pool,
                    determinism: &mut determinism,
                    modifiers: &mut modifiers,
                    target: &mut target,
                },
                &ParsedBehavior::new(100036, "SkillChangeAttackCareer", vec![1]),
            ),
            Some(Vec::new())
        );
        assert_eq!(modifiers.attack_career, Some(1));
    }

    #[test]
    fn career_restraint_waits_for_the_hit_target_before_selecting_a_career() {
        let fight = sonettobuf::Fight {
            defender: Some(sonettobuf::FightTeam {
                entitys: vec![sonettobuf::FightEntityInfo {
                    uid: Some(-1),
                    current_hp: Some(100),
                    career: Some(1),
                    weak_careers: vec![3],
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let managers = crate::engine::manager::BattleManagers::seeded(&fight);
        let pool = crate::engine::skill::target::TargetPool::from_fight(&fight);
        let mut determinism = crate::engine::runtime::determinism::RoundDeterminism::default();
        let mut modifiers = crate::engine::skill::action::SkillModifiers::default();
        let mut target = crate::engine::skill::target::TargetContext {
            hit_target_uid: -1,
            ..Default::default()
        };
        let behavior = ParsedBehavior::new(60299, "SetCareerRestraint", vec![]);

        assert_eq!(
            Handler::emit_ops(
                BehaviorOpContext {
                    source_uid: 10,
                    source_team: 1,
                    target_uid: 10,
                    active_skill_id: 20,
                    transfer_count: 1,
                    event: None,
                    managers: &managers,
                    pool: &pool,
                    determinism: &mut determinism,
                    modifiers: &mut modifiers,
                    target: &mut target,
                },
                &behavior,
            ),
            Some(Vec::new())
        );
        assert_eq!(modifiers.attack_career, None);
        assert!(Handler::collect_attack_modifier(
            AttackModifierContext {
                operation: BehaviorOpContext {
                    source_uid: 10,
                    source_team: 1,
                    target_uid: 0,
                    active_skill_id: 20,
                    transfer_count: 1,
                    event: None,
                    managers: &managers,
                    pool: &pool,
                    determinism: &mut determinism,
                    modifiers: &mut modifiers,
                    target: &mut target,
                },
                conditions: &[],
            },
            &behavior,
        ));
        assert_eq!(modifiers.attack_career, Some(3));
    }
}

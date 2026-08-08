use crate::engine::manager::buff::ActiveBuffFeature;

use super::{is_kind, registry::BuffActKind};

pub fn supports(args: &[i32]) -> bool {
    args.is_empty()
}

pub fn skill_uses_action_point(
    features: &[ActiveBuffFeature],
    owner_uid: i64,
    is_big_skill: bool,
) -> bool {
    if is_big_skill {
        return super::big_skill_no_use_action_point::skill_uses_action_point(
            features, owner_uid, true,
        );
    }
    !features.iter().any(|feature| {
        feature.owner_uid == owner_uid && is_kind(feature, BuffActKind::SkillNoUseActPoint)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feature(kind: &str, act_id: i32) -> ActiveBuffFeature {
        ActiveBuffFeature {
            owner_uid: 1,
            source_uid: 1,
            buff_uid: 2,
            buff_id: 3,
            amount: 1,
            team_type: 1,
            owner_alive: true,
            act_type: kind.to_owned(),
            effect_time: 0,
            effect_condition: 0,
            raw: String::new(),
            values: vec![act_id],
        }
    }

    #[test]
    fn waives_basic_incantations_only_for_the_buff_owner() {
        let feature = feature("SkillNoUseActPoint", 1140);
        assert!(!skill_uses_action_point(&[feature.clone()], 1, false));
        assert!(skill_uses_action_point(&[feature.clone()], 1, true));
        assert!(skill_uses_action_point(&[feature], 2, false));
    }

    #[test]
    fn delegates_ultimate_waivers_to_the_existing_ultimate_buff() {
        let feature = feature("BigSkillNoUseActPoint", 946);
        assert!(!skill_uses_action_point(&[feature], 1, true));
    }
}

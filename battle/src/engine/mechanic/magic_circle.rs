use sonettobuf::{Fight, MagicCircleInfo};

use crate::engine::{entity::attr::AttrId, skill::target::TargetPool};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MagicCircleApplyResult {
    pub target_uid: i64,
    pub circle_id: i32,
    pub info: MagicCircleInfo,
}

#[derive(Debug, Clone, Default)]
pub struct MagicCircle {
    current: Option<MagicCircleInfo>,
}

impl MagicCircle {
    pub fn seed_from_start(fight: &Fight) -> Self {
        Self {
            current: fight.magic_circle,
        }
    }

    pub fn current_id(&self) -> i32 {
        self.current
            .as_ref()
            .and_then(|circle| circle.magic_circle_id)
            .unwrap_or_default()
    }

    pub fn current_source_uid(&self) -> i64 {
        self.current
            .as_ref()
            .and_then(|circle| circle.create_uid)
            .unwrap_or_default()
    }

    pub fn add(
        &mut self,
        source_uid: i64,
        target_uid: i64,
        circle_id: i32,
        level: i32,
    ) -> Option<MagicCircleApplyResult> {
        let row = config::try_get()?.magic_circle.get(circle_id)?;
        let info = MagicCircleInfo {
            magic_circle_id: Some(circle_id),
            round: Some(row.round),
            create_uid: Some(source_uid),
            electric_level: Some(level.max(0)),
            electric_progress: Some(0),
            max_electric_progress: Some(if row.circle_type == 1 && level > 0 {
                90
            } else {
                0
            }),
        };
        self.current = Some(info);
        Some(MagicCircleApplyResult {
            target_uid,
            circle_id,
            info,
        })
    }

    pub fn set_progress(
        &mut self,
        target_uid: i64,
        progress: i32,
    ) -> Option<MagicCircleApplyResult> {
        let info = self.current.as_mut()?;
        info.electric_progress = Some(progress.max(0));
        Some(MagicCircleApplyResult {
            target_uid: info
                .create_uid
                .filter(|uid| *uid != 0)
                .unwrap_or(target_uid),
            circle_id: info.magic_circle_id.unwrap_or_default(),
            info: *info,
        })
    }

    pub fn attribute_delta(&self, uid: i64, attr_id: AttrId, pool: &TargetPool) -> i32 {
        let Some(circle) = self.current.as_ref() else {
            return 0;
        };
        let Some(row) = circle
            .magic_circle_id
            .and_then(|id| config::try_get()?.magic_circle.get(id))
        else {
            return 0;
        };
        let creator = circle.create_uid.unwrap_or_default();
        let raw = if pool.source_is_attacker(uid) == pool.source_is_attacker(creator) {
            &row.self_attrs
        } else {
            &row.enemy_attrs
        };
        let values = raw
            .split(['#', '|'])
            .filter_map(|value| value.trim().parse::<i32>().ok())
            .collect::<Vec<_>>();
        values
            .chunks_exact(2)
            .filter_map(|pair| (pair[0] == attr_id as i32).then_some(pair[1]))
            .sum()
    }
}

pub fn linked_buffs(circle_id: i32) -> (Vec<i32>, Vec<i32>) {
    let Some(row) = config::try_get().and_then(|db| db.magic_circle.get(circle_id)) else {
        return (Vec::new(), Vec::new());
    };
    let mut ally = parse_ids(&row.self_buff);
    ally.extend(
        row.complex_effect
            .split('|')
            .filter_map(|entry| entry.split_once(':').map(|(_, value)| value))
            .filter_map(|value| value.split(',').nth(2))
            .flat_map(parse_ids),
    );
    ally.sort_unstable();
    ally.dedup();
    (ally, parse_ids(&row.enemy_buff))
}

fn parse_ids(raw: &str) -> Vec<i32> {
    raw.split(['|', '#'])
        .filter_map(|id| id.trim().parse().ok())
        .filter(|id| *id > 0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blood_domain_links_its_ally_healing_buff() {
        crate::test_support::init_config();

        assert_eq!(linked_buffs(100051), (vec![308801312], Vec::new()));
    }

    #[test]
    fn pulsing_field_attributes_come_from_its_magic_circle_row() {
        crate::test_support::init_config();
        let fight = Fight {
            attacker: Some(sonettobuf::FightTeam {
                entitys: vec![sonettobuf::FightEntityInfo {
                    uid: Some(10),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            defender: Some(sonettobuf::FightTeam {
                entitys: vec![sonettobuf::FightEntityInfo {
                    uid: Some(-1),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut circle = MagicCircle::default();
        circle.add(10, 10, 30001, 1);
        let pool = TargetPool::from_fight(&fight);

        assert_eq!(circle.attribute_delta(10, AttrId::DmgBonus, &pool), 150);
        assert_eq!(circle.attribute_delta(-1, AttrId::DmgBonus, &pool), 0);
    }
}

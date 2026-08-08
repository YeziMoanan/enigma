use crate::engine::manager::field::{FieldDefinition, FieldThreshold};
use crate::engine::mechanic::impromptu::ImpromptuDefinition;
use crate::engine::skill::rule::{CommandOrigin, RuleDomain};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MagicCircleDefinition {
    pub duration: i32,
    pub allied_attributes: Vec<(i32, i32)>,
    pub enemy_attributes: Vec<(i32, i32)>,
    pub allied_buffs: Vec<i32>,
    pub enemy_buffs: Vec<i32>,
    pub self_skills: Vec<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LingeringGlowAttributeBuff {
    pub buff_id: i32,
    pub origin: CommandOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfiguredFightVersion {
    Missing,
    Invalid,
    Value(i32),
}

#[derive(Clone, Copy)]
pub struct BattleCatalog {
    game_data: &'static config::GameDB,
    fight_version: ConfiguredFightVersion,
    impromptu_definition: Option<ImpromptuDefinition>,
    lingering_glow_attribute_buff: Option<LingeringGlowAttributeBuff>,
}

impl BattleCatalog {
    pub fn new(game_data: &'static config::GameDB) -> Self {
        Self {
            game_data,
            fight_version: configured_fight_version(
                game_data.r#const.get(1707).map(|row| row.value.as_str()),
            ),
            impromptu_definition: impromptu_definition(game_data),
            lingering_glow_attribute_buff: lingering_glow_attribute_buff(game_data),
        }
    }

    pub(crate) fn game_data(self) -> &'static config::GameDB {
        self.game_data
    }

    pub(crate) fn fight_version(self) -> ConfiguredFightVersion {
        self.fight_version
    }

    pub(crate) fn lingering_glow_attribute_buff(self) -> Option<LingeringGlowAttributeBuff> {
        self.lingering_glow_attribute_buff
    }

    pub(crate) fn impromptu_definition(self) -> Option<ImpromptuDefinition> {
        self.impromptu_definition
    }

    pub(crate) fn magic_circle(self, circle_id: i32) -> Option<MagicCircleDefinition> {
        magic_circle_definition(self.game_data, circle_id)
    }

    pub(crate) fn magic_circle_thresholds(self) -> Vec<FieldThreshold> {
        self.game_data
            .fight_dnsz
            .iter()
            .filter_map(|threshold| {
                let circle = self.game_data.magic_circle.get(threshold.id)?;
                Some(FieldThreshold {
                    level: threshold.level,
                    progress: threshold.progress,
                    definition: FieldDefinition {
                        field_id: threshold.id,
                        duration: circle.round,
                    },
                })
            })
            .collect()
    }

    pub(crate) fn buff_has_effect_count(self, buff_id: i32) -> bool {
        self.game_data
            .skill_buff
            .get(buff_id)
            .is_some_and(|buff| buff.effect_count > 0)
    }

    pub(crate) fn buff_expires_after_owner_attack(self, buff_id: i32) -> bool {
        let Some(buff) = self.game_data.skill_buff.get(buff_id) else {
            return false;
        };
        let type_id = if buff.type_id == 0 {
            buff.id
        } else {
            buff.type_id
        };
        self.game_data
            .skill_bufftype
            .get(type_id)
            .is_some_and(|buff_type| buff_type.take_act == "1")
    }

    pub(crate) fn try_global() -> Option<Self> {
        config::try_get().map(Self::new)
    }
}

fn magic_circle_definition(
    game_data: &config::GameDB,
    circle_id: i32,
) -> Option<MagicCircleDefinition> {
    let row = game_data.magic_circle.get(circle_id)?;
    Some(MagicCircleDefinition {
        duration: row.round,
        allied_attributes: parse_attribute_pairs(&row.self_attrs),
        enemy_attributes: parse_attribute_pairs(&row.enemy_attrs),
        allied_buffs: parse_positive_ids(&row.self_buff),
        enemy_buffs: parse_positive_ids(&row.enemy_buff),
        self_skills: parse_positive_ids(&row.self_skills),
    })
}

fn parse_attribute_pairs(raw: &str) -> Vec<(i32, i32)> {
    parse_integers(raw)
        .chunks_exact(2)
        .map(|pair| (pair[0], pair[1]))
        .collect()
}

fn parse_positive_ids(raw: &str) -> Vec<i32> {
    parse_integers(raw)
        .into_iter()
        .filter(|id| *id > 0)
        .collect()
}

fn parse_integers(raw: &str) -> Vec<i32> {
    raw.split(['|', '#'])
        .filter_map(|value| value.trim().parse().ok())
        .collect()
}

fn configured_fight_version(raw: Option<&str>) -> ConfiguredFightVersion {
    let Some(raw) = raw else {
        return ConfiguredFightVersion::Missing;
    };
    raw.parse()
        .map(ConfiguredFightVersion::Value)
        .unwrap_or(ConfiguredFightVersion::Invalid)
}

pub(crate) fn impromptu_definition(game_data: &config::GameDB) -> Option<ImpromptuDefinition> {
    Some(ImpromptuDefinition::new(
        game_data.fight_asfd_const.get(5)?.value.parse().ok()?,
        game_data.buff_act.iter().find_map(|act| {
            let definition = crate::engine::skill::buff_act::registry::find(act.id, &act.r#type)?;
            (definition.kind
                == crate::engine::skill::buff_act::registry::BuffActKind::EmitterDamageUp)
                .then_some(definition.key.opcode)
        })?,
        game_data.fight_asfd_const.get(6)?.value.parse().ok()?,
    ))
}

fn lingering_glow_attribute_buff(game_data: &config::GameDB) -> Option<LingeringGlowAttributeBuff> {
    let buff_id = game_data
        .fight_jgz_const
        .get(2)?
        .value
        .parse::<i32>()
        .ok()?;
    let buff = game_data.skill_buff.get(buff_id)?;
    let origin = lingering_glow_attribute_origin(game_data, &buff.features)?;
    Some(LingeringGlowAttributeBuff { buff_id, origin })
}

fn lingering_glow_attribute_origin(
    game_data: &config::GameDB,
    features: &str,
) -> Option<CommandOrigin> {
    features.split('|').find_map(|feature| {
        let values = feature
            .split('#')
            .map(|value| value.trim().parse::<i32>())
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        let (&act_id, args) = values.split_first()?;
        let act = game_data.buff_act.get(act_id)?;
        let definition = crate::engine::skill::buff_act::registry::find(act_id, &act.r#type)?;
        (definition.kind == crate::engine::skill::buff_act::registry::BuffActKind::AttrByHeatScale
            && definition.supports.is_some_and(|supports| supports(args)))
        .then_some(CommandOrigin {
            domain: RuleDomain::BuffAct,
            key: definition.key,
        })
    })
}

impl std::fmt::Debug for BattleCatalog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("BattleCatalog")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_lingering_glow_attribute_buff() {
        crate::test_support::init_config();

        assert_eq!(
            BattleCatalog::new(crate::test_support::game_data()).lingering_glow_attribute_buff(),
            Some(LingeringGlowAttributeBuff {
                buff_id: 31340007,
                origin: CommandOrigin {
                    domain: RuleDomain::BuffAct,
                    key: crate::engine::skill::rule::DefinitionKey::new(1053, "AttrByHeatScale"),
                },
            })
        );
    }

    #[test]
    fn normalizes_fight_version() {
        crate::test_support::init_config();
        let game_data = crate::test_support::game_data();

        assert_eq!(
            BattleCatalog::new(game_data).fight_version(),
            ConfiguredFightVersion::Value(
                game_data.r#const.get(1707).unwrap().value.parse().unwrap()
            )
        );
        assert_eq!(
            configured_fight_version(None),
            ConfiguredFightVersion::Missing
        );
        assert_eq!(
            configured_fight_version(Some("not-an-integer")),
            ConfiguredFightVersion::Invalid
        );
    }

    #[test]
    fn normalizes_impromptu_definition() {
        crate::test_support::init_config();
        let game_data = crate::test_support::game_data();
        let catalog = BattleCatalog::new(game_data);
        let definition = catalog.impromptu_definition().unwrap();

        assert_eq!(
            definition.skill_id(),
            game_data
                .fight_asfd_const
                .get(5)
                .unwrap()
                .value
                .parse::<i32>()
                .unwrap()
        );
        assert_eq!(
            game_data
                .buff_act
                .get(definition.damage_up_act_id())
                .unwrap()
                .r#type,
            "EmitterDamageUp"
        );
        assert_eq!(
            definition.damage_rate(2),
            game_data
                .fight_asfd_const
                .get(6)
                .unwrap()
                .value
                .parse::<i32>()
                .unwrap()
                * 2
        );
    }

    #[test]
    fn normalizes_magic_circle_attributes_and_thresholds() {
        crate::test_support::init_config();
        let catalog = BattleCatalog::new(crate::test_support::game_data());

        assert_eq!(
            catalog.magic_circle(30001),
            Some(MagicCircleDefinition {
                duration: 3,
                allied_attributes: vec![(205, 150)],
                enemy_attributes: Vec::new(),
                allied_buffs: Vec::new(),
                enemy_buffs: Vec::new(),
                self_skills: Vec::new(),
            })
        );
        assert_eq!(
            catalog.magic_circle_thresholds(),
            vec![
                FieldThreshold {
                    level: 1,
                    progress: 0,
                    definition: FieldDefinition {
                        field_id: 30001,
                        duration: 3,
                    },
                },
                FieldThreshold {
                    level: 2,
                    progress: 50,
                    definition: FieldDefinition {
                        field_id: 30002,
                        duration: 3,
                    },
                },
                FieldThreshold {
                    level: 3,
                    progress: 120,
                    definition: FieldDefinition {
                        field_id: 30003,
                        duration: 2,
                    },
                },
            ]
        );
    }

    #[test]
    fn normalizes_magic_circle_linked_battle_rules() {
        crate::test_support::init_config();

        let blood_domain = BattleCatalog::new(crate::test_support::game_data())
            .magic_circle(100051)
            .unwrap();

        assert_eq!(blood_domain.allied_buffs, vec![308801312]);
        assert_eq!(blood_domain.self_skills, vec![308801821]);
    }

    #[test]
    fn normalizes_buff_consumption_and_action_expiry() {
        crate::test_support::init_config();
        let catalog = BattleCatalog::new(crate::test_support::game_data());

        assert!(catalog.buff_has_effect_count(6240530));
        assert!(!catalog.buff_has_effect_count(610091));
        assert!(catalog.buff_has_effect_count(90201));
        assert!(!catalog.buff_expires_after_owner_attack(90201));
        assert!(catalog.buff_expires_after_owner_attack(2220010));
        assert!(!catalog.buff_has_effect_count(-1));
        assert!(!catalog.buff_expires_after_owner_attack(-1));
    }

    #[test]
    fn rejects_unsupported_lingering_glow_attribute_buff() {
        crate::test_support::init_config();
        let game_data = crate::test_support::game_data();

        assert_eq!(
            lingering_glow_attribute_origin(game_data, "1053#201#0#1000000"),
            None
        );
        assert!(lingering_glow_attribute_origin(game_data, "1053#201#5#1000000#10000").is_some());
    }
}

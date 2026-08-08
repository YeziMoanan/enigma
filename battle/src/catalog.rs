use crate::engine::skill::rule::{CommandOrigin, RuleDomain};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LingeringGlowAttributeBuff {
    pub buff_id: i32,
    pub origin: CommandOrigin,
}

#[derive(Clone, Copy)]
pub struct BattleCatalog {
    game_data: &'static config::GameDB,
    lingering_glow_attribute_buff: Option<LingeringGlowAttributeBuff>,
}

impl BattleCatalog {
    pub fn new(game_data: &'static config::GameDB) -> Self {
        Self {
            game_data,
            lingering_glow_attribute_buff: lingering_glow_attribute_buff(game_data),
        }
    }

    pub(crate) fn game_data(self) -> &'static config::GameDB {
        self.game_data
    }

    pub(crate) fn lingering_glow_attribute_buff(self) -> Option<LingeringGlowAttributeBuff> {
        self.lingering_glow_attribute_buff
    }
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

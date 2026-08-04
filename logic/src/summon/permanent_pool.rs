use serde::{Deserialize, Serialize};

pub const SOURCE_DATA_SHA: &str = "04ef16b69e0508fe7d62671327350ada222e7323";

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct PermanentPoolSnapshot {
    pub source_data_sha: String,
    pub included: Vec<PermanentHero>,
    pub excluded: Vec<ExcludedHero>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct PermanentHero {
    pub id: i32,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct ExcludedHero {
    pub id: i32,
    pub name: String,
    pub reason: ExclusionReason,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    Offline,
    NonPlayable,
    MissingSkin,
    MissingSkill,
}

pub fn eligible_six_stars(tables: &config::GameDB) -> PermanentPoolSnapshot {
    let mut included = Vec::new();
    let mut excluded = Vec::new();

    for character in tables.character.iter().filter(|row| row.rare == 5) {
        let name = if character.name_eng.trim().is_empty() {
            character.name.clone()
        } else {
            character.name_eng.clone()
        };
        let reason = exclusion_reason(tables, character);
        if let Some(reason) = reason {
            excluded.push(ExcludedHero {
                id: character.id,
                name,
                reason,
            });
        } else {
            included.push(PermanentHero {
                id: character.id,
                name,
            });
        }
    }

    included.sort_by_key(|hero| hero.id);
    excluded.sort_by_key(|hero| hero.id);
    PermanentPoolSnapshot {
        source_data_sha: SOURCE_DATA_SHA.to_owned(),
        included,
        excluded,
    }
}

fn exclusion_reason(
    tables: &config::GameDB,
    character: &config::character::Character,
) -> Option<ExclusionReason> {
    if character.is_online != "1" {
        return Some(ExclusionReason::Offline);
    }
    if character.hero_type <= 0 {
        return Some(ExclusionReason::NonPlayable);
    }
    if character.skin_id == 0
        || !tables
            .skin
            .get(character.skin_id)
            .is_some_and(|skin| skin.character_id == character.id)
    {
        return Some(ExclusionReason::MissingSkin);
    }
    if character.skill.trim().is_empty()
        || character.ex_skill == 0
        || !referenced_skills(character).all(|skill_id| tables.skill.get(skill_id).is_some())
        || tables.skill.get(character.ex_skill).is_none()
    {
        return Some(ExclusionReason::MissingSkill);
    }
    None
}

fn referenced_skills(character: &config::character::Character) -> impl Iterator<Item = i32> + '_ {
    character
        .skill
        .split(['|', '#', ','])
        .filter_map(|value| value.parse::<i32>().ok())
        .filter(|value| *value > 1_000)
}

#[cfg(test)]
mod tests {
    #[test]
    fn pinned_data_selects_only_playable_online_six_stars() {
        let data_dir = format!("{}/../data/excel2json", env!("CARGO_MANIFEST_DIR"));
        let _ = config::init(&data_dir);

        let snapshot = super::eligible_six_stars(config::configs::get());
        let included = snapshot
            .included
            .iter()
            .map(|hero| hero.id)
            .collect::<Vec<_>>();

        assert!(included.contains(&3143));
        for excluded in [3136, 3144, 3145, 3146, 3147] {
            assert!(!included.contains(&excluded));
        }
        assert!(
            snapshot
                .included
                .windows(2)
                .all(|pair| pair[0].id < pair[1].id)
        );
        assert!(
            snapshot
                .excluded
                .windows(2)
                .all(|pair| pair[0].id < pair[1].id)
        );
    }
}

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
pub enum MailCategory {
    Currency,
    Material,
    Consumable,
    Equipment,
    Psychube,
    Skin,
    Cloth,
    Wilderness,
    Antique,
    Character,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct CatalogEntry {
    pub material_type: i32,
    pub id: i32,
    pub name: String,
    pub category: MailCategory,
    pub quantity: i32,
}

pub fn build_initial_catalog(db: &config::GameDB) -> Vec<CatalogEntry> {
    let mut entries = Vec::new();

    for row in db.currency.all() {
        if row.id > 0 && !row.name.trim().is_empty() && !row.icon.trim().is_empty() {
            entries.push(entry(2, row.id, &row.name, MailCategory::Currency, 9_999));
        }
    }
    for row in db.item.all() {
        if row.id > 0
            && row.is_show == 1
            && row.expire_time.trim().is_empty()
            && !row.name.trim().is_empty()
            && !row.icon.trim().is_empty()
            && row.activity_id == 0
        {
            entries.push(entry(
                1,
                row.id,
                &row.name,
                if row.is_stackable == 1 {
                    MailCategory::Material
                } else {
                    MailCategory::Consumable
                },
                if row.is_stackable == 1 { 9_999 } else { 1 },
            ));
        }
    }
    for row in db.power_item.all() {
        if row.id > 0 && !row.name.trim().is_empty() && !row.icon.trim().is_empty() {
            entries.push(entry(10, row.id, &row.name, MailCategory::Consumable, 1));
        }
    }
    for row in db.insight_item.all() {
        if row.id > 0 && !row.name.trim().is_empty() && !row.icon.trim().is_empty() {
            entries.push(entry(24, row.id, &row.name, MailCategory::Material, 1));
        }
    }
    for row in db.equip.all() {
        if row.id > 0 && !row.name.trim().is_empty() && !row.icon.trim().is_empty() {
            entries.push(entry(9, row.id, &row.name, MailCategory::Psychube, 1));
        }
    }
    for row in db.skin.all() {
        if row.id > 0 && row.character_id > 0 && !row.name.trim().is_empty() {
            entries.push(entry(5, row.id, &row.name, MailCategory::Skin, 1));
        }
    }
    for row in db.room_building.all() {
        if row.id > 0 && !row.name.trim().is_empty() {
            entries.push(entry(11, row.id, &row.name, MailCategory::Wilderness, 1));
        }
    }
    for row in db.block_package.all() {
        if row.id > 0 && !row.show_only && !row.name.trim().is_empty() {
            entries.push(entry(13, row.id, &row.name, MailCategory::Wilderness, 1));
        }
    }
    for row in db.antique.all() {
        if row.id > 0 && !row.name.trim().is_empty() && !row.icon.trim().is_empty() {
            entries.push(entry(18, row.id, &row.name, MailCategory::Antique, 1));
        }
    }
    let cloth_ids = db
        .reward_group
        .all()
        .iter()
        .filter(|row| row.material_type == 7 && row.material_id > 0)
        .map(|row| row.material_id)
        .collect::<BTreeSet<_>>();
    for id in cloth_ids {
        entries.push(entry(7, id, &format!("服装 {id}"), MailCategory::Cloth, 1));
    }
    let special_block_ids = db
        .reward_group
        .all()
        .iter()
        .filter(|row| row.material_type == 14 && row.material_id > 0)
        .map(|row| row.material_id)
        .collect::<BTreeSet<_>>();
    for id in special_block_ids {
        entries.push(entry(
            14,
            id,
            &format!("特殊地块 {id}"),
            MailCategory::Wilderness,
            1,
        ));
    }
    if let Some(row) = db.character.get(3143) {
        entries.push(entry(
            4,
            row.id,
            character_mail_name(row.id, &row.name),
            MailCategory::Character,
            1,
        ));
    }

    entries.sort_by_key(|entry| (entry.category, entry.material_type, entry.id));
    entries.dedup_by_key(|entry| (entry.material_type, entry.id));
    entries
}

fn character_mail_name(id: i32, fallback: &str) -> &str {
    match id {
        3143 => "哑谜",
        _ => fallback,
    }
}

fn entry(
    material_type: i32,
    id: i32,
    name: &str,
    category: MailCategory,
    quantity: i32,
) -> CatalogEntry {
    CatalogEntry {
        material_type,
        id,
        name: name.trim().to_string(),
        category,
        quantity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn config() -> &'static config::GameDB {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("data")
            .join("excel2json");
        let _ = config::init(path.to_str().unwrap());
        config::configs::get()
    }

    #[test]
    fn catalog_is_safe_and_contains_only_the_allowed_new_character() {
        let catalog = build_initial_catalog(config());
        assert!(
            catalog
                .iter()
                .any(|entry| entry.material_type == 4 && entry.id == 3143)
        );
        assert!(
            catalog
                .iter()
                .all(|entry| entry.quantity == 1 || entry.quantity == 9_999)
        );
        assert!(
            !catalog
                .iter()
                .any(|entry| matches!(entry.material_type, 3 | 25 | 29))
        );
        assert!(
            catalog
                .iter()
                .filter(|entry| entry.material_type == 4)
                .all(|entry| entry.id == 3143)
        );
        assert_eq!(
            catalog
                .iter()
                .find(|entry| entry.material_type == 4 && entry.id == 3143)
                .map(|entry| entry.name.as_str()),
            Some("哑谜")
        );
    }

    #[test]
    fn catalog_ids_are_unique_within_each_reward_type() {
        let catalog = build_initial_catalog(config());
        let mut ids = HashSet::new();
        for entry in catalog {
            assert!(ids.insert((entry.material_type, entry.id)));
        }
    }

    #[test]
    fn non_stackable_catalog_entries_always_grant_one() {
        let db = config();
        for entry in build_initial_catalog(db) {
            let non_stackable = match entry.material_type {
                1 => db
                    .item
                    .get(entry.id)
                    .is_some_and(|item| item.is_stackable != 1),
                4 | 5 | 7 | 10 | 24 => true,
                9 => db
                    .equip
                    .get(entry.id)
                    .is_some_and(|equip| equip.is_exp_equip != 1),
                _ => false,
            };
            if non_stackable {
                assert_eq!(
                    entry.quantity, 1,
                    "non-stackable material {}#{} must grant exactly one",
                    entry.material_type, entry.id
                );
            }
        }
    }
}

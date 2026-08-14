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
            entries.push(entry(
                2,
                row.id,
                localized_name(db, &row.name, 2, row.id),
                MailCategory::Currency,
                9_999,
            ));
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
                localized_name(db, &row.name, 1, row.id),
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
            entries.push(entry(
                10,
                row.id,
                localized_name(db, &row.name, 10, row.id),
                MailCategory::Consumable,
                1,
            ));
        }
    }
    for row in db.insight_item.all() {
        if row.id > 0 && !row.name.trim().is_empty() && !row.icon.trim().is_empty() {
            entries.push(entry(
                24,
                row.id,
                localized_name(db, &row.name, 24, row.id),
                MailCategory::Material,
                1,
            ));
        }
    }
    for row in db.equip.all() {
        if row.id > 0 && !row.name.trim().is_empty() && !row.icon.trim().is_empty() {
            entries.push(entry(
                9,
                row.id,
                localized_name(db, &row.name, 9, row.id),
                MailCategory::Psychube,
                1,
            ));
        }
    }
    for row in db.skin.all() {
        if row.id > 0 && row.character_id > 0 && !row.name.trim().is_empty() {
            entries.push(entry(
                5,
                row.id,
                localized_name(db, &row.name, 5, row.id),
                MailCategory::Skin,
                1,
            ));
        }
    }
    for row in db.room_building.all() {
        if row.id > 0 && !row.name.trim().is_empty() {
            entries.push(entry(
                11,
                row.id,
                localized_name(db, &row.name, 11, row.id),
                MailCategory::Wilderness,
                1,
            ));
        }
    }
    for row in db.block_package.all() {
        if row.id > 0 && !row.show_only && !row.name.trim().is_empty() {
            entries.push(entry(
                13,
                row.id,
                localized_name(db, &row.name, 13, row.id),
                MailCategory::Wilderness,
                1,
            ));
        }
    }
    for row in db.antique.all() {
        if row.id > 0 && !row.name.trim().is_empty() && !row.icon.trim().is_empty() {
            entries.push(entry(
                18,
                row.id,
                localized_name(db, &row.name, 18, row.id),
                MailCategory::Antique,
                1,
            ));
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
            localized_name(db, &row.name, 4, row.id),
            MailCategory::Character,
            1,
        ));
    }

    entries.sort_by_key(|entry| (entry.category, entry.material_type, entry.id));
    entries.dedup_by_key(|entry| (entry.material_type, entry.id));
    entries
}

fn entry(
    material_type: i32,
    id: i32,
    name: impl Into<String>,
    category: MailCategory,
    quantity: i32,
) -> CatalogEntry {
    CatalogEntry {
        material_type,
        id,
        name: name.into().trim().to_string(),
        category,
        quantity,
    }
}

/// The international data tables store display names as language keys. The
/// public Chinese admin must never expose those implementation keys to users.
/// Keep the protocol/data IDs unchanged and translate only the presentation
/// name; unknown keys receive a stable Chinese fallback instead of leaking the
/// raw `language_xxx` token.
fn localized_name(_db: &config::GameDB, raw: &str, material_type: i32, id: i32) -> String {
    if !raw.starts_with("language_") {
        return raw.trim().to_string();
    }
    let known = match raw {
        "language_10003211" => Some("纯雨滴"),
        "language_10003214" => Some("澄澈雨滴"),
        "language_10003217" => Some("利齿子儿"),
        "language_10003220" => Some("细胞活性"),
        "language_10003223" => Some("微尘"),
        "language_10003226" => Some("迷途之齿"),
        "language_10003229" => Some("迷途之齿唱片"),
        "language_10003233" => Some("思绪点"),
        "language_10003236" => Some("全知之书"),
        "language_10003239" => Some("梦境流体"),
        "language_10003242" => Some("荒原贝壳"),
        "language_10003245" => Some("阅读概率"),
        "language_10003248" => Some("UTTU代币"),
        "language_10003251" => Some("小狗硬币"),
        "language_10003254" => Some("尖叫罐头"),
        "language_10003257" => Some("干木材"),
        "language_10003261" => Some("永恒星锑"),
        "language_10003264" => Some("闪耀之物"),
        "language_10003268" => Some("旧日的金匣"),
        "language_10003271" => Some("归途券"),
        "language_10003274" => Some("尤里卡"),
        "language_10003278" => Some("苹果币"),
        "language_10003281" => Some("尘封文件"),
        "language_10003284" => Some("火花印章"),
        "language_10003287" => Some("远古火种"),
        "language_10003290" => Some("桉树果"),
        "language_10003295" => Some("纸马"),
        "language_10003298" => Some("光明的馈赠"),
        "language_10003305" => Some("幸运币"),
        "language_10033894" => Some("UTTU积分"),
        "language_10033889" => Some("交响"),
        "language_10036862" => Some("修复零件"),
        "language_10036864" => Some("修复材料"),
        "language_10036866" => Some("基石"),
        "language_10036870" => Some("黑钻石"),
        "language_10036873" => Some("原始组件"),
        _ => None,
    };
    // The public Chinese admin must not leak the English localization table.
    // Unknown entries keep their stable protocol identity and use a Chinese
    // fallback until a reviewed localized mapping is added.
    known
        .map(str::to_string)
        .unwrap_or_else(|| format!("物品 {material_type}:{id}"))
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

    #[test]
    fn catalog_names_are_chinese_or_explicit_fallbacks() {
        let catalog = build_initial_catalog(config());
        assert!(catalog.iter().any(|entry| entry.name == "微尘"));
        assert!(
            catalog
                .iter()
                .all(|entry| !entry.name.starts_with("language_"))
        );
    }
}

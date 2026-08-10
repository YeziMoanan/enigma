use super::catalog::{CatalogEntry, MailCategory, build_initial_catalog};
use serde::{Deserialize, Serialize};

pub const INITIAL_MAIL_BODY: &str =
    "该游戏服务器纯公益无收费，如果收费携带邮件举报卖家\n群号：1084799028";

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct InitialMailManifest {
    pub campaign_id: String,
    pub source_data_sha: String,
    pub mails: Vec<ManifestMail>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct ManifestMail {
    pub sequence: i32,
    pub category: MailCategory,
    pub title: String,
    pub body: String,
    pub attachment: String,
    pub entries: Vec<CatalogEntry>,
}

pub fn build_initial_manifest(
    db: &config::GameDB,
    source_data_sha: impl Into<String>,
    campaign_id: impl Into<String>,
) -> InitialMailManifest {
    build_manifest_from_entries(build_initial_catalog(db), source_data_sha, campaign_id)
}

pub fn build_manifest_from_entries(
    mut entries: Vec<CatalogEntry>,
    source_data_sha: impl Into<String>,
    campaign_id: impl Into<String>,
) -> InitialMailManifest {
    entries.sort_by_key(|entry| (entry.category, entry.material_type, entry.id));
    let mut mails = Vec::new();
    for category in [
        MailCategory::Currency,
        MailCategory::Material,
        MailCategory::Consumable,
        MailCategory::Equipment,
        MailCategory::Psychube,
        MailCategory::Skin,
        MailCategory::Cloth,
        MailCategory::Wilderness,
        MailCategory::Antique,
        MailCategory::Character,
    ] {
        let category_entries = entries
            .iter()
            .filter(|entry| entry.category == category)
            .cloned()
            .collect::<Vec<_>>();
        for (index, chunk) in category_entries.chunks(5).enumerate() {
            let chunk = chunk.to_vec();
            let sequence = mails.len() as i32 + 1;
            let category_sequence = index as i32 + 1;
            mails.push(ManifestMail {
                sequence,
                category,
                title: format!("{}-{category_sequence}", category.title()),
                body: INITIAL_MAIL_BODY.to_string(),
                attachment: chunk
                    .iter()
                    .map(|entry| format!("{}#{}#{}", entry.material_type, entry.id, entry.quantity))
                    .collect::<Vec<_>>()
                    .join("|"),
                entries: chunk,
            });
        }
    }
    InitialMailManifest {
        campaign_id: campaign_id.into(),
        source_data_sha: source_data_sha.into(),
        mails,
    }
}

impl MailCategory {
    pub const fn title(self) -> &'static str {
        match self {
            Self::Currency => "货币",
            Self::Material => "材料",
            Self::Consumable => "消耗品",
            Self::Equipment => "装备",
            Self::Psychube => "心相",
            Self::Skin => "皮肤",
            Self::Cloth => "服装",
            Self::Wilderness => "荒原",
            Self::Antique => "藏品",
            Self::Character => "角色",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries() -> Vec<CatalogEntry> {
        (0..11)
            .map(|id| CatalogEntry {
                material_type: 1,
                id,
                name: format!("item-{id}"),
                category: MailCategory::Material,
                quantity: 9_999,
            })
            .chain(std::iter::once(CatalogEntry {
                material_type: 4,
                id: 3143,
                name: "character".to_string(),
                category: MailCategory::Character,
                quantity: 1,
            }))
            .collect()
    }

    #[test]
    fn manifest_is_deterministic_and_limits_each_mail_to_five_entries() {
        let first = build_manifest_from_entries(entries(), "sha", "initial-full-v1");
        let second = build_manifest_from_entries(entries(), "sha", "initial-full-v1");
        assert_eq!(first, second);
        assert!(first.mails.iter().all(|mail| {
            (1..=5).contains(&mail.entries.len())
                && mail.entries.len() == mail.attachment.split('|').count()
                && mail.title.contains('-')
                && mail.body == INITIAL_MAIL_BODY
        }));
        assert_eq!(
            first
                .mails
                .iter()
                .filter(|mail| mail.category == MailCategory::Material)
                .map(|mail| mail.sequence)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(
            first
                .mails
                .iter()
                .map(|mail| mail.sequence)
                .collect::<Vec<_>>(),
            (1..=first.mails.len() as i32).collect::<Vec<_>>()
        );
    }
}

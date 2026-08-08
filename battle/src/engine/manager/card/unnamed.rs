use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sonettobuf::{CardData, CardInfo, card_data::CardDataKey};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnnamedCardData {
    pub lock: bool,
    pub strengthen: BTreeMap<String, i32>,
}

impl Default for UnnamedCardData {
    fn default() -> Self {
        Self {
            lock: true,
            strengthen: (1..=4).map(|track| (track.to_string(), 0)).collect(),
        }
    }
}

impl UnnamedCardData {
    pub const MAX_STRENGTHEN: i32 = 4;

    pub fn from_card(card: &CardInfo) -> Option<Self> {
        let value = card
            .card_dataes
            .iter()
            .find(|data| data.key == Some(CardDataKey::Unnamed as i32))?
            .value
            .as_deref()?;
        serde_json::from_str(value).ok()
    }

    pub fn write_to(&self, card: &mut CardInfo) -> bool {
        let Ok(value) = serde_json::to_string(self) else {
            return false;
        };
        if let Some(data) = card
            .card_dataes
            .iter_mut()
            .find(|data| data.key == Some(CardDataKey::Unnamed as i32))
        {
            data.value = Some(value);
        } else {
            card.card_dataes.push(CardData {
                key: Some(CardDataKey::Unnamed as i32),
                value: Some(value),
            });
        }
        true
    }

    pub fn strengthen(&mut self, track: i32, amount: i32) -> bool {
        if !(1..=4).contains(&track) || amount <= 0 {
            return false;
        }
        let value = self.strengthen.entry(track.to_string()).or_default();
        let next = value.saturating_add(amount).min(Self::MAX_STRENGTHEN);
        if next == *value {
            return false;
        }
        *value = next;
        true
    }
}

pub fn is_unnamed(card: &CardInfo) -> bool {
    UnnamedCardData::from_card(card).is_some()
}

pub fn is_locked(card: &CardInfo) -> bool {
    UnnamedCardData::from_card(card).is_some_and(|data| data.lock)
}

pub fn caster_uid(card: &CardInfo) -> i64 {
    card.uid
        .filter(|uid| *uid != 0)
        .or(card.target_uid.filter(|uid| *uid != 0))
        .unwrap_or_default()
}

pub fn card(owner_uid: i64, skill_id: i32) -> Option<CardInfo> {
    if owner_uid == 0 || skill_id <= 0 {
        return None;
    }
    let mut card = CardInfo {
        uid: Some(0),
        target_uid: Some(owner_uid),
        skill_id: Some(skill_id),
        temp_card: Some(false),
        ..Default::default()
    };
    UnnamedCardData::default()
        .write_to(&mut card)
        .then_some(card)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unnamed_card_round_trips_lock_and_strengthen_data() {
        let mut card = card(42, 31470131).unwrap();
        let mut data = UnnamedCardData::from_card(&card).unwrap();
        assert!(data.lock);
        assert_eq!(data.strengthen.get("1"), Some(&0));

        data.lock = false;
        assert!(data.strengthen(1, 2));
        assert!(data.write_to(&mut card));
        assert_eq!(UnnamedCardData::from_card(&card), Some(data));
        assert_eq!(caster_uid(&card), 42);
    }

    #[test]
    fn strengthen_is_capped_at_four_layers() {
        let mut data = UnnamedCardData::default();
        assert!(data.strengthen(3, 16));
        assert_eq!(data.strengthen.get("3"), Some(&4));
        assert!(!data.strengthen(3, 1));
    }
}

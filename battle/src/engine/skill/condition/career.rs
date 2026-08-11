use crate::engine::skill::condition::parse::{ConditionCompare, ParsedConditionKind};

pub fn target_career(_: i32, _: &str, args: &[String]) -> Option<ParsedConditionKind> {
    Some(ParsedConditionKind::TargetCareer(parse_args(args)?))
}

pub fn parse_career_check(_: i32, _: &str, args: &[String]) -> Option<ParsedConditionKind> {
    Some(ParsedConditionKind::TargetSharesCasterCareer {
        param: args.first().and_then(|arg| arg.parse().ok()).unwrap_or(0),
    })
}

pub fn parse_per_target_career_count(
    _: i32,
    _: &str,
    args: &[String],
) -> Option<ParsedConditionKind> {
    Some(ParsedConditionKind::PerTargetCareerCount {
        careers: parse_list(args.first()?)?,
        threshold: args.last().and_then(|arg| arg.parse().ok()).unwrap_or(0),
    })
}

pub fn team_career_count_at_least(_: i32, _: &str, args: &[String]) -> Option<ParsedConditionKind> {
    Some(ParsedConditionKind::TeamCareerCount {
        careers: parse_list(args.first()?)?,
        compare: ConditionCompare::GreaterThanOrEqual,
        threshold: args.get(1)?.parse().ok()?,
    })
}

pub fn team_career_count_at_most(_: i32, _: &str, args: &[String]) -> Option<ParsedConditionKind> {
    Some(ParsedConditionKind::TeamCareerCount {
        careers: parse_list(args.first()?)?,
        compare: ConditionCompare::LessThanOrEqual,
        threshold: args.get(1)?.parse().ok()?,
    })
}

pub fn team_career_count_equal(_: i32, _: &str, args: &[String]) -> Option<ParsedConditionKind> {
    Some(ParsedConditionKind::TeamCareerCount {
        careers: parse_list(args.first()?)?,
        compare: ConditionCompare::Equal,
        threshold: args.get(1)?.parse().ok()?,
    })
}

pub fn natural_ally_count(_: i32, _: &str, args: &[String]) -> Option<ParsedConditionKind> {
    Some(ParsedConditionKind::PerTargetCareerCount {
        careers: vec![1, 2, 3, 4],
        threshold: args.first()?.parse().ok()?,
    })
}

fn parse_args(args: &[String]) -> Option<Vec<i32>> {
    args.iter().map(|arg| arg.parse().ok()).collect()
}

fn parse_list(raw: &str) -> Option<Vec<i32>> {
    raw.split([',', '，'])
        .map(|part| part.trim().parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_parsers_construct_their_owned_kind() {
        assert!(matches!(
            target_career(16002, "TargetCareer", &["3".into()]),
            Some(ParsedConditionKind::TargetCareer(_))
        ));
        assert!(matches!(
            parse_career_check(508212, "CareerCheck", &[]),
            Some(ParsedConditionKind::TargetSharesCasterCareer { param: 0 })
        ));
    }

    #[test]
    fn battle_rule_career_filter_keeps_all_configured_afflatuses() {
        assert_eq!(
            target_career(
                16204,
                "TargetCareer",
                &["1".into(), "2".into(), "3".into(), "4".into()],
            ),
            Some(ParsedConditionKind::TargetCareer(vec![1, 2, 3, 4]))
        );
    }

    #[test]
    fn natural_ally_count_keeps_the_configured_cap() {
        assert_eq!(
            natural_ally_count(621002, "CareerNatureHeroNum", &["3".into()]),
            Some(ParsedConditionKind::PerTargetCareerCount {
                careers: vec![1, 2, 3, 4],
                threshold: 3,
            })
        );
    }

    #[test]
    fn team_career_threshold_keeps_the_configured_group_and_minimum() {
        assert_eq!(
            team_career_count_at_least(562002, "CareerGroupHeroCountGE", &["3".into(), "3".into()],),
            Some(ParsedConditionKind::TeamCareerCount {
                careers: vec![3],
                compare: ConditionCompare::GreaterThanOrEqual,
                threshold: 3,
            })
        );
        assert_eq!(
            team_career_count_at_most(
                560100,
                "CareerGroupHeroCountLE",
                &["3,5,6".into(), "2".into()],
            ),
            Some(ParsedConditionKind::TeamCareerCount {
                careers: vec![3, 5, 6],
                compare: ConditionCompare::LessThanOrEqual,
                threshold: 2,
            })
        );
        assert_eq!(
            team_career_count_equal(
                561100,
                "CareerGroupHeroCountEqual",
                &["3,5,6".into(), "1".into()],
            ),
            Some(ParsedConditionKind::TeamCareerCount {
                careers: vec![3, 5, 6],
                compare: ConditionCompare::Equal,
                threshold: 1,
            })
        );
    }
}

use crate::engine::skill::condition::parse::{ParsedConditionKind, parse_fixed};

pub fn ex_point(_: i32, _: &str, args: &[String]) -> Option<ParsedConditionKind> {
    let [compare_code, threshold] = parse_fixed(args)?;
    Some(ParsedConditionKind::ConduitExPoint {
        compare_code,
        threshold,
    })
}

pub fn selected_group(_: i32, _: &str, args: &[String]) -> Option<ParsedConditionKind> {
    let [group] = parse_fixed(args)?;
    (group > 0).then_some(ParsedConditionKind::ConduitSkillGroup { group })
}

pub fn counter(_: i32, _: &str, args: &[String]) -> Option<ParsedConditionKind> {
    let [counter_id, divisor, max_count] = args else {
        return None;
    };
    let counter_id = counter_id.parse().ok()?;
    let divisor = divisor.parse().ok()?;
    let max_count = max_count.parse().ok()?;
    (counter_id > 0 && divisor > 0 && max_count > 0).then_some(
        ParsedConditionKind::ConduitCounter {
            counter_id,
            divisor,
            max_count,
        },
    )
}

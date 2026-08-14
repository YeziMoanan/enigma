use super::permanent_pool::eligible_six_stars;
use super::*;

const PERMILLE: f64 = 1_000.0;

#[derive(Clone, Copy)]
enum SummonRateConfig {
    SixStarUp = 805,
    FiveStarUp = 806,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SummonType {
    Newbie,
    Normal,
    ProbUp,
    MultiProbUp4,
    LuckyBag,
    Limit,
    CustomPick,
    StrongCustomOnePick,
    CoBranding,
    NewPlayer,
    DoubleSsrUp,
    Other(i32),
}

impl From<i32> for SummonType {
    fn from(value: i32) -> Self {
        match value {
            1 => Self::Newbie,
            2 => Self::Normal,
            3 => Self::ProbUp,
            4 => Self::MultiProbUp4,
            5 => Self::LuckyBag,
            6 => Self::Limit,
            7 => Self::CustomPick,
            12 => Self::StrongCustomOnePick,
            21 => Self::CoBranding,
            201 => Self::NewPlayer,
            202 => Self::DoubleSsrUp,
            value => Self::Other(value),
        }
    }
}

impl SummonType {
    pub(super) fn uses_sp_pool_info(self) -> bool {
        matches!(self, Self::StrongCustomOnePick | Self::CoBranding)
    }
}

#[derive(Clone)]
pub(crate) struct GachaPool {
    six_up: Vec<i32>,
    six_up_weighted: Vec<(i32, u32)>,
    six_normal: Vec<i32>,
    five_up: Vec<i32>,
    five_normal: Vec<i32>,
    four: Vec<i32>,
    three: Vec<i32>,
    two: Vec<i32>,
}

pub(super) struct GachaState {
    pub(super) pity_6: u32,
    pub(super) up_guaranteed: bool,
}

pub(super) struct GachaRules {
    rarity_weights: Vec<(i32, u32)>,
    soft_pity: u32,
    hard_pity: u32,
    six_rate_step: f64,
    six_up_rate: f64,
    five_up_rate: f64,
}

pub(super) struct GachaResult {
    pub(super) hero_id: i32,
}

impl GachaRules {
    pub(super) fn from_pool(pool: &config::summon_pool::SummonPool) -> Result<Self, AppError> {
        let mut rules = Self::from_values(
            pool.r#type,
            &pool.init_weight,
            &pool.award_time,
            &pool.change_weight,
        )?;
        rules.six_up_rate = configured_six_up_rate(pool)?;
        rules.five_up_rate = (!parse_up_heroes(&pool.up_weight).1.is_empty())
            .then(|| configured_common_rate(SummonRateConfig::FiveStarUp))
            .transpose()?
            .unwrap_or_default();
        Ok(rules)
    }

    pub(super) fn from_values(
        summon_type: i32,
        init_weight: &str,
        award_time: &str,
        change_weight: &str,
    ) -> Result<Self, AppError> {
        let rarity_weights = parse_weighted(init_weight);
        if !rarity_weights.iter().any(|(rarity, _)| *rarity == 5)
            || !rarity_weights.iter().any(|(rarity, _)| *rarity != 5)
        {
            return Err(AppError::InvalidRequest);
        }

        let mut award_times = award_time
            .split('|')
            .filter_map(|value| value.parse::<u32>().ok());
        let soft_pity = award_times.next().ok_or(AppError::InvalidRequest)?;
        let hard_pity = award_times.next().ok_or(AppError::InvalidRequest)?;
        let configured_step = parse_weighted(change_weight)
            .into_iter()
            .find_map(|(rarity, weight)| (rarity == 5).then_some(weight))
            .unwrap_or_default() as f64
            / 10_000.0;

        Ok(Self {
            rarity_weights,
            soft_pity,
            hard_pity,
            six_rate_step: if SummonType::from(summon_type) == SummonType::LuckyBag {
                configured_step
            } else {
                configured_step / 2.0
            },
            six_up_rate: 0.0,
            five_up_rate: 0.0,
        })
    }

    pub(super) fn six_rate(&self, pity: u32) -> f64 {
        if pity >= self.hard_pity {
            return 1.0;
        }

        let base = self
            .rarity_weights
            .iter()
            .find_map(|(rarity, weight)| (*rarity == 5).then_some(*weight as f64 / 10_000.0))
            .unwrap_or_default();
        base + pity.saturating_sub(self.soft_pity) as f64 * self.six_rate_step
    }

    fn lower_rarity(&self, rng: &mut impl Rng) -> i32 {
        let weights = self
            .rarity_weights
            .iter()
            .copied()
            .filter(|(rarity, _)| *rarity != 5)
            .collect::<Vec<_>>();
        choose_weighted(rng, &weights)
    }
}

fn configured_six_up_rate(pool: &config::summon_pool::SummonPool) -> Result<f64, AppError> {
    let summon_type = SummonType::from(pool.r#type);
    let (six_up, _) = parse_up_heroes(&pool.up_weight);
    let rate = match summon_type {
        SummonType::MultiProbUp4 if !six_up.is_empty() => 1.0,
        SummonType::CustomPick => pool
            .param
            .split_once('|')
            .map(|(_, rates)| parse_ids(rates).into_iter().sum::<i32>() as f64 / PERMILLE)
            .unwrap_or_default(),
        SummonType::StrongCustomOnePick => configured_common_rate(SummonRateConfig::SixStarUp)?,
        SummonType::DoubleSsrUp => {
            parse_weighted(&pool.double_ssr_up_rates)
                .into_iter()
                .map(|(_, rate)| rate)
                .sum::<u32>() as f64
                / PERMILLE
        }
        SummonType::ProbUp | SummonType::Limit | SummonType::CoBranding | SummonType::NewPlayer
            if !six_up.is_empty() =>
        {
            configured_common_rate(SummonRateConfig::SixStarUp)?
        }
        _ => 0.0,
    };
    (0.0..=1.0)
        .contains(&rate)
        .then_some(rate)
        .ok_or(AppError::InvalidRequest)
}

fn configured_common_rate(config: SummonRateConfig) -> Result<f64, AppError> {
    config::configs::get()
        .r#const
        .get(config as i32)
        .and_then(|row| row.value.parse::<f64>().ok())
        .map(|rate| rate / PERMILLE)
        .filter(|rate| (0.0..=1.0).contains(rate))
        .ok_or(AppError::InvalidRequest)
}

impl GachaState {
    pub(super) fn single_pull(
        &mut self,
        rules: &GachaRules,
        pool: &GachaPool,
        rng: &mut impl Rng,
        force_five: bool,
    ) -> GachaResult {
        self.pity_6 += 1;
        let six_rate = rules.six_rate(self.pity_6);

        if rng.random::<f64>() < six_rate {
            self.pity_6 = 0;
            let has_up = !pool.six_up.is_empty();
            let is_up = has_up && (self.up_guaranteed || rng.random_bool(rules.six_up_rate));
            self.up_guaranteed = has_up && !is_up;
            let hero_id = pool.choose_top_hero(rng, is_up).unwrap();
            return GachaResult { hero_id };
        }

        let rarity = if force_five {
            4
        } else {
            rules.lower_rarity(rng)
        };
        let hero_id = match rarity {
            4 if !pool.five_up.is_empty() && rng.random_bool(rules.five_up_rate) => {
                *pool.five_up.choose(rng).unwrap()
            }
            4 => *pool.five_normal.choose(rng).unwrap(),
            3 => *pool.four.choose(rng).unwrap(),
            2 => *pool.three.choose(rng).unwrap(),
            _ => *pool.two.choose(rng).unwrap(),
        };

        GachaResult { hero_id }
    }

    pub(super) fn ten_pull(
        &mut self,
        rules: &GachaRules,
        pool: &GachaPool,
        rng: &mut impl Rng,
    ) -> Vec<GachaResult> {
        let mut results = vec![self.single_pull(rules, pool, rng, true)];
        for _ in 1..10 {
            results.push(self.single_pull(rules, pool, rng, false));
        }
        results
    }
}

impl GachaPool {
    fn choose_top_hero(&self, rng: &mut impl Rng, prefer_up: bool) -> Result<i32, AppError> {
        if prefer_up && !self.six_up_weighted.is_empty() {
            return Ok(choose_weighted(rng, &self.six_up_weighted));
        }

        if prefer_up && !self.six_up.is_empty() {
            return self
                .six_up
                .choose(rng)
                .copied()
                .ok_or(AppError::InvalidRequest);
        }

        self.six_normal
            .choose(rng)
            .or_else(|| self.six_up.choose(rng))
            .copied()
            .ok_or(AppError::InvalidRequest)
    }

    pub(crate) fn choose_config_rarity(
        &self,
        rarity: i32,
        rng: &mut impl Rng,
    ) -> Result<i32, AppError> {
        match rarity {
            5 => {
                let prefer_up = !self.six_up.is_empty() && rng.random_bool(0.5);
                self.choose_top_hero(rng, prefer_up)
            }
            4 if !self.five_up.is_empty() && rng.random_bool(0.5) => self
                .five_up
                .choose(rng)
                .copied()
                .ok_or(AppError::InvalidRequest),
            4 => self
                .five_normal
                .choose(rng)
                .copied()
                .ok_or(AppError::InvalidRequest),
            3 => self
                .four
                .choose(rng)
                .copied()
                .ok_or(AppError::InvalidRequest),
            2 => self
                .three
                .choose(rng)
                .copied()
                .ok_or(AppError::InvalidRequest),
            1 => self
                .two
                .choose(rng)
                .copied()
                .ok_or(AppError::InvalidRequest),
            _ => Err(AppError::InvalidRequest),
        }
    }
}

pub(crate) fn build_gacha_pool(
    pool_id: i32,
    sp_pool: Option<&database::models::game::summon::SpPoolInfo>,
) -> Result<GachaPool, AppError> {
    let tables = config::configs::get();
    let pool_cfg = tables
        .summon_pool
        .iter()
        .find(|pool| pool.id == pool_id)
        .ok_or(AppError::InvalidRequest)?;
    let summon_type = SummonType::from(sp_pool.map(|sp| sp.sp_type).unwrap_or(pool_cfg.r#type));

    let (six_up, five_up, six_up_weighted) = match summon_type {
        SummonType::CustomPick | SummonType::StrongCustomOnePick => (
            sp_pool
                .map(|sp| sp.up_hero_ids.clone())
                .filter(|ids| !ids.is_empty())
                .ok_or(AppError::InvalidRequest)?,
            Vec::new(),
            Vec::new(),
        ),
        SummonType::DoubleSsrUp => {
            let (six, five) = parse_up_heroes(&pool_cfg.up_weight);
            (six, five, parse_weighted(&pool_cfg.double_ssr_up_rates))
        }
        SummonType::Normal => (Vec::new(), Vec::new(), Vec::new()),
        _ => {
            let (six, five) = parse_up_heroes(&pool_cfg.up_weight);
            (six, five, Vec::new())
        }
    };

    let mut six_all = Vec::new();
    let mut five_all = Vec::new();
    let mut four = Vec::new();
    let mut three = Vec::new();
    let mut two = Vec::new();

    for row in tables.summon_entries(pool_id) {
        let ids = parse_ids(&row.summon_id);
        match row.rare {
            5 => six_all.extend(ids),
            4 => five_all.extend(ids),
            3 => four.extend(ids),
            2 => three.extend(ids),
            1 => two.extend(ids),
            _ => {}
        }
    }

    if summon_type == SummonType::Normal {
        six_all = eligible_six_stars(tables)
            .included
            .into_iter()
            .map(|hero| hero.id)
            .collect();
        for character in tables.character.iter().filter(|character| {
            character.is_online == "1"
                && character.hero_type > 0
                && character.skin_id > 0
                && !character.skill.trim().is_empty()
                && character.ex_skill > 0
        }) {
            match character.rare {
                4 => five_all.push(character.id),
                3 => four.push(character.id),
                2 => three.push(character.id),
                1 => two.push(character.id),
                _ => {}
            }
        }
        for ids in [&mut six_all, &mut five_all, &mut four, &mut three, &mut two] {
            ids.sort_unstable();
            ids.dedup();
        }
    }

    Ok(GachaPool {
        six_normal: six_all
            .into_iter()
            .filter(|id| !six_up.contains(id))
            .collect(),
        five_normal: five_all
            .into_iter()
            .filter(|id| !five_up.contains(id))
            .collect(),
        six_up,
        six_up_weighted,
        five_up,
        four,
        three,
        two,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::summon::permanent_pool::eligible_six_stars;

    #[test]
    fn normal_pool_contains_exact_permanent_roster() {
        let data_dir = format!("{}/../data/excel2json", env!("CARGO_MANIFEST_DIR"));
        let _ = config::init(&data_dir);
        let tables = config::configs::get();
        let normal = tables
            .summon_pool
            .iter()
            .find(|pool| SummonType::from(pool.r#type) == SummonType::Normal)
            .expect("normal summon pool");
        let pool = build_gacha_pool(normal.id, None).expect("build normal pool");
        let expected = eligible_six_stars(tables)
            .included
            .into_iter()
            .map(|hero| hero.id)
            .collect::<Vec<_>>();

        assert_eq!(pool.six_normal, expected);
        assert_eq!(pool.six_normal.len(), 70);
        assert_eq!(pool.five_normal.len(), 29);
        assert_eq!(pool.four.len(), 18);
        assert_eq!(pool.three.len(), 11);
        assert_eq!(pool.two.len(), 2);
        assert_eq!(
            pool.six_normal.len()
                + pool.five_normal.len()
                + pool.four.len()
                + pool.three.len()
                + pool.two.len(),
            130
        );

        for included_id in [3120, 3140, 3144, 3145, 3146, 3147] {
            assert!(pool.six_normal.contains(&included_id));
        }

        let all_ids = pool
            .six_normal
            .iter()
            .chain(&pool.five_normal)
            .chain(&pool.four)
            .chain(&pool.three)
            .chain(&pool.two)
            .copied()
            .collect::<Vec<_>>();
        for excluded_id in [3029, 3136, 9998] {
            assert!(!all_ids.contains(&excluded_id));
        }

        assert_eq!(tables.character.get(3146).unwrap().name_eng, "Rhiannon");
        assert_eq!(tables.character.get(3147).unwrap().name_eng, "Ms. Stranger");
    }

    #[tokio::test]
    async fn newly_added_permanent_heroes_can_be_created() {
        let data_dir = format!("{}/../data/excel2json", env!("CARGO_MANIFEST_DIR"));
        let _ = config::init(&data_dir);
        let db = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        database::run_migrations(&db).await.unwrap();
        sqlx::query(
            "INSERT INTO users (id, username, created_at, updated_at)
             VALUES (71, 'permanent-pool-all', 0, 0)",
        )
        .execute(&db)
        .await
        .unwrap();
        let heroes = database::models::game::heros::UserHeroModel::new(71, db);

        for hero_id in [3120, 3140, 3144, 3145, 3146, 3147] {
            heroes.create_hero(hero_id).await.unwrap();
        }
    }

    #[test]
    fn normal_pool_keeps_configured_probability_and_pity() {
        let data_dir = format!("{}/../data/excel2json", env!("CARGO_MANIFEST_DIR"));
        let _ = config::init(&data_dir);
        let normal = config::configs::get()
            .summon_pool
            .iter()
            .find(|pool| SummonType::from(pool.r#type) == SummonType::Normal)
            .expect("normal summon pool");
        let rules = GachaRules::from_pool(normal).expect("normal summon rules");

        assert_eq!(rules.rarity_weights, parse_weighted(&normal.init_weight));
        assert_eq!(rules.soft_pity, 60);
        assert_eq!(rules.hard_pity, 70);
        assert_eq!(rules.six_rate(0), 0.015);
        assert_eq!(rules.six_rate(60), 0.015);
        assert_eq!(rules.six_rate(61), 0.04);
        assert_eq!(rules.six_rate(70), 1.0);
    }

    #[test]
    fn non_normal_pools_keep_their_configured_candidates() {
        let data_dir = format!("{}/../data/excel2json", env!("CARGO_MANIFEST_DIR"));
        let _ = config::init(&data_dir);
        let tables = config::configs::get();

        for pool_cfg in tables.summon_pool.iter() {
            let summon_type = SummonType::from(pool_cfg.r#type);
            if matches!(
                summon_type,
                SummonType::Normal | SummonType::CustomPick | SummonType::StrongCustomOnePick
            ) {
                continue;
            }
            let Ok(pool) = build_gacha_pool(pool_cfg.id, None) else {
                continue;
            };
            let (six_up, five_up, six_up_weighted) = if summon_type == SummonType::DoubleSsrUp {
                let (six, five) = parse_up_heroes(&pool_cfg.up_weight);
                (six, five, parse_weighted(&pool_cfg.double_ssr_up_rates))
            } else {
                let (six, five) = parse_up_heroes(&pool_cfg.up_weight);
                (six, five, Vec::new())
            };
            let mut by_rarity = std::collections::BTreeMap::<i32, Vec<i32>>::new();
            for row in tables.summon_entries(pool_cfg.id) {
                by_rarity
                    .entry(row.rare)
                    .or_default()
                    .extend(parse_ids(&row.summon_id));
            }

            assert_eq!(pool.six_up, six_up, "pool {} six_up", pool_cfg.id);
            assert_eq!(
                pool.six_up_weighted, six_up_weighted,
                "pool {} six_up_weighted",
                pool_cfg.id
            );
            assert_eq!(pool.five_up, five_up, "pool {} five_up", pool_cfg.id);
            assert_eq!(
                pool.six_normal,
                by_rarity
                    .remove(&5)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|id| !pool.six_up.contains(id))
                    .collect::<Vec<_>>(),
                "pool {} six_normal",
                pool_cfg.id
            );
            assert_eq!(
                pool.five_normal,
                by_rarity
                    .remove(&4)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|id| !pool.five_up.contains(id))
                    .collect::<Vec<_>>(),
                "pool {} five_normal",
                pool_cfg.id
            );
            assert_eq!(pool.four, by_rarity.remove(&3).unwrap_or_default());
            assert_eq!(pool.three, by_rarity.remove(&2).unwrap_or_default());
            assert_eq!(pool.two, by_rarity.remove(&1).unwrap_or_default());
        }
    }
}

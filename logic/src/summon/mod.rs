use crate::{error::AppError, reward};
use database::{
    db::{
        game::{guides, summon},
        user::account,
    },
    models::game::{currencies::UserCurrencyModel, heros::UserHeroModel, items::UserItemModel},
};
use rand::{Rng, prelude::IndexedRandom};
use sonettobuf::{
    ChooseEnhancedPoolHeroReply, ChooseMultiUpHeroReply, EndActivityPush, GetSummonInfoReply,
    GetSummonProgressRewardsReply, GuideInfo, PopUpRecommendWindowReply, SummonQueryTokenReply,
    SummonReply, SummonResult,
};
use sqlx::SqlitePool;
mod commands;
mod parse;
pub mod permanent_pool;
mod pool;

pub use commands::SummonCompletion;
use parse::{choose_weighted, parse_ids, parse_up_heroes, parse_weighted};
pub(crate) use pool::build_gacha_pool;
use pool::{GachaResult, GachaRules, GachaState, SummonType};

#[derive(Clone, Copy, Debug)]
pub struct SummonManager {
    player_id: i64,
}

impl SummonManager {
    pub fn new(player_id: i64) -> Self {
        Self { player_id }
    }

    pub async fn info(&self, db: &SqlitePool) -> Result<GetSummonInfoReply, AppError> {
        summon::sync_visible_pools(db, self.player_id).await?;
        commands::summon_info(db, self.player_id).await
    }

    pub async fn progress_rewards(
        &self,
        db: &SqlitePool,
        pool_id: i32,
    ) -> Result<(GetSummonProgressRewardsReply, Vec<u32>), AppError> {
        commands::progress_rewards(db, self.player_id, pool_id).await
    }

    pub async fn pop_up_recommend_window(
        &self,
        db: &SqlitePool,
        pool_id: i32,
        order_id: i32,
    ) -> Result<PopUpRecommendWindowReply, AppError> {
        commands::pop_up_recommend_window(db, self.player_id, pool_id, order_id).await
    }

    pub async fn query_token(
        &self,
        db: &SqlitePool,
    ) -> Result<(SummonQueryTokenReply, EndActivityPush), AppError> {
        commands::query_token(db, self.player_id).await
    }

    pub async fn summon(
        &self,
        db: &SqlitePool,
        pool_id: i32,
        guide_id: Option<i32>,
        step_id: Option<i32>,
        count: i32,
    ) -> Result<SummonCompletion, AppError> {
        commands::summon(db, self.player_id, pool_id, guide_id, step_id, count).await
    }

    pub async fn choose_enhanced_pool_hero(
        &self,
        db: &SqlitePool,
        pool_id: i32,
        hero_id: i32,
    ) -> Result<ChooseEnhancedPoolHeroReply, AppError> {
        commands::choose_enhanced_pool_hero(db, self.player_id, pool_id, hero_id).await
    }

    pub async fn choose_multi_up_hero(
        &self,
        db: &SqlitePool,
        pool_id: i32,
        hero_ids: Vec<i32>,
    ) -> Result<ChooseMultiUpHeroReply, AppError> {
        commands::choose_multi_up_hero(db, self.player_id, pool_id, hero_ids).await
    }
}

#[cfg(test)]
mod test;

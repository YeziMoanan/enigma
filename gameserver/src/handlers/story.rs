use crate::{
    error::AppError,
    net::{context::ConnectionContext, packet::ClientPacket},
    util::push,
};
use prost::Message;
use sonettobuf::{
    CmdId, FinishNecrologistStoryModeReply, FinishNecrologistStoryModeRequest, GetHeroStoryRequest,
    GetNecrologistStoryRequest, GetStoryFinishRequest, StoryFinishPush,
    UpdateNecrologistStoryRequest, UpdateStoryRequest,
};

pub async fn on_get_story(ctx: &mut ConnectionContext, req: ClientPacket) -> Result<(), AppError> {
    let reply = ctx.player()?.story.get(ctx.state.db).await?;
    ctx.send_reply(CmdId::GetStoryCmd, reply, 0, req.up_tag)
        .await
}

pub async fn on_get_story_finish(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = GetStoryFinishRequest::decode(&req.data[..])?;
    let reply = ctx
        .player()?
        .story
        .finish_state(ctx.state.db, msg.story_id.ok_or(AppError::InvalidRequest)?)
        .await?;
    ctx.send_reply(CmdId::GetStoryFinishCmd, reply, 0, req.up_tag)
        .await
}

pub async fn on_update_story(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let player_id = ctx.player()?.id;
    let msg = UpdateStoryRequest::decode(&req.data[..])?;
    let update = ctx
        .player()?
        .story
        .update(
            ctx.state.db,
            msg.story_id.unwrap_or_default(),
            msg.step_id.unwrap_or_default(),
            msg.favor.unwrap_or_default(),
        )
        .await?;
    if let Some(story_id) = update.finished_story_id {
        ctx.notify(
            CmdId::StoryFinishPushCmd,
            StoryFinishPush {
                story_id: Some(story_id),
            },
        )
        .await?;
        if let Some((chapter_id, episode_id)) = battleless_story_completion(story_id)
            && database::db::game::dungeons::episode_star(ctx.state.db, player_id, episode_id)
                .await?
                == 0
        {
            let settlement = crate::dungeon::settle_battleless(
                ctx.state.db,
                player_id,
                chapter_id,
                episode_id,
                crate::dungeon::DungeonCompletion {
                    star: 1,
                    total_round: 0,
                    multiplier: 1,
                    fight_group: None,
                },
                &Default::default(),
            )
            .await?;
            push::send_cost_pushes(
                ctx,
                player_id,
                settlement.cost.item_ids,
                settlement.cost.currency_ids,
            )
            .await?;
            super::dungeon::send_completed_dungeon(
                ctx,
                player_id,
                chapter_id,
                episode_id,
                settlement.dungeon,
            )
            .await?;
        }
        push::send_dungeon_map_progression(ctx, player_id).await?;
    }
    ctx.send_reply(CmdId::UpdateStoryCmd, update.reply, 0, req.up_tag)
        .await
}

fn battleless_story_completion(story_id: i32) -> Option<(i32, i32)> {
    const CURTAIN_CALL_STORY_ID: i32 = 100743;
    const CURTAIN_CALL_EPISODE_ID: i32 = 10730;

    if story_id != CURTAIN_CALL_STORY_ID {
        return None;
    }
    let episode = config::configs::get()
        .episode
        .get(CURTAIN_CALL_EPISODE_ID)?;
    (episode.before_story == story_id && episode.battle_id == 0)
        .then_some((episode.chapter_id, episode.id))
}

pub async fn on_get_hero_story(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    GetHeroStoryRequest::decode(&req.data[..])?;
    let reply = ctx.player()?.story.hero_story(ctx.state.db).await?;
    ctx.send_reply(CmdId::GetHeroStoryCmd, reply, 0, req.up_tag)
        .await
}

pub async fn on_get_necrologist_story(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = GetNecrologistStoryRequest::decode(&req.data[..])?;
    let reply = ctx
        .player()?
        .story
        .necrologist_story(
            ctx.state.db,
            msg.story_id.unwrap_or_default(),
            ctx.state.tables,
        )
        .await?;
    ctx.send_reply(CmdId::GetNecrologistStoryCmd, reply, 0, req.up_tag)
        .await
}

pub async fn on_update_necrologist_story(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = UpdateNecrologistStoryRequest::decode(&req.data[..])?;
    let reply = ctx
        .player()?
        .story
        .update_necrologist_story(
            ctx.state.db,
            msg.story_id.unwrap_or_default(),
            msg.info.unwrap_or_else(|| "{}".to_string()),
            msg.plot_infos,
        )
        .await?;
    ctx.send_reply(CmdId::UpdateNecrologistStoryCmd, reply, 0, req.up_tag)
        .await
}

pub async fn on_finish_necrologist_story_mode(
    ctx: &mut ConnectionContext,
    req: ClientPacket,
) -> Result<(), AppError> {
    let msg = FinishNecrologistStoryModeRequest::decode(&req.data[..])?;
    let reply = FinishNecrologistStoryModeReply {
        story_id: msg.story_id,
        mode_id: msg.mode_id,
    };
    ctx.send_reply(CmdId::FinishNecrologistStoryModeCmd, reply, 0, req.up_tag)
        .await
}

#[cfg(test)]
mod tests {
    use super::battleless_story_completion;

    #[test]
    fn vereinsamt_curtain_call_finishes_with_its_story() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("data")
            .join("excel2json");
        let _ = config::init(path.to_str().unwrap());

        assert_eq!(battleless_story_completion(100743), Some((107, 10730)));
        assert_eq!(battleless_story_completion(100742), None);
    }
}

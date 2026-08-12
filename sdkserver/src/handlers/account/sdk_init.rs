use crate::models::response::{
    AccountSdkInitRsp, AccountSdkInitRspData, BizSwitch, GameChannel, ShowButtons, UserCenterItem,
};
use axum::{extract::Query, response::Json};
use std::collections::HashMap;

pub async fn post(query: Query<HashMap<String, String>>) -> Json<AccountSdkInitRsp> {
    let has_query = !query.is_empty();

    let data = if has_query {
        AccountSdkInitRspData {
            game_channel: Some(GameChannel {
                game_id: 60001,
                channel_id: 200,
                cp_name: "重返未来：1999".to_string(),
                app_id: "1".to_string(),
                app_key: "1".to_string(),
                call_interval: 600,
                relogin_interval: 60,
                relogin_times: 5,
                is_record_debug: false,
            }),
            biz_switch: Some(BizSwitch {
                open_real_name_window: false,
                force_real_name_auth: false,
            }),
            is_download_service: Some(true),
            is_show_stop_service_baffle: Some(false),
            is_ignore_file_missing: Some(false),
            is_open_c_m_p: Some(false),
            show_buttons: Some(ShowButtons { notice: true }),
            // 私服仅支持邮箱账号登录；Private server supports email login only.
            login_account_types: Some(vec![10]),
            user_center_items: None,
            only_mail: Some(true),
            is_unsupport_change_volume: false,
        }
    } else {
        AccountSdkInitRspData {
            // 私服仅支持邮箱账号登录；Private server supports email login only.
            login_account_types: Some(vec![10]),
            user_center_items: Some(vec![
                UserCenterItem {
                    r#type: 1,
                    lab_title: "账号管理".to_string(),
                },
                UserCenterItem {
                    r#type: 2,
                    lab_title: "客服".to_string(),
                },
                UserCenterItem {
                    r#type: 3,
                    lab_title: "隐私条约".to_string(),
                },
                UserCenterItem {
                    r#type: 4,
                    lab_title: "账号注销".to_string(),
                },
            ]),
            only_mail: Some(true),
            is_unsupport_change_volume: false,
            game_channel: None,
            biz_switch: None,
            is_download_service: None,
            is_show_stop_service_baffle: None,
            is_ignore_file_missing: None,
            is_open_c_m_p: None,
            show_buttons: None,
        }
    };

    let rsp = AccountSdkInitRsp {
        code: 200,
        msg: "success".to_string(),
        data,
    };

    Json(rsp)
}

#[cfg(test)]
mod tests {
    use super::post;
    use axum::extract::Query;
    use std::collections::HashMap;

    #[tokio::test]
    async fn both_init_branches_expose_only_email_login() {
        let without_query = post(Query(HashMap::new())).await.0;
        assert_eq!(without_query.data.login_account_types, Some(vec![10]));
        assert_eq!(without_query.data.only_mail, Some(true));

        let mut query = HashMap::new();
        query.insert("channel".to_string(), "200".to_string());
        let with_query = post(Query(query)).await.0;
        assert_eq!(with_query.data.login_account_types, Some(vec![10]));
        assert_eq!(with_query.data.only_mail, Some(true));
    }
}

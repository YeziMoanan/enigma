use crate::AppState;
use crate::models::request::PaymentListReq;
use crate::models::response::{PaymentListRsp, PaymentListRspData};
use axum::extract::State;
use axum::response::Json;

pub async fn post(
    State(_): State<AppState>,
    axum::Json(req): axum::Json<PaymentListReq>,
) -> Json<PaymentListRsp> {
    tracing::info!("Received payment list request for user: {}", req.user_id);

    let response = PaymentListRsp {
        code: 200,
        msg: "success".to_string(),
        data: PaymentListRspData {
            payments: Vec::new(),
            web_pre_pay_url: String::new(),
        },
    };

    tracing::info!("Payments are disabled");

    Json(response)
}

#[cfg(test)]
mod tests {
    use super::post;
    use crate::{AppState, SdkState};
    use axum::{Json, extract::State};
    use common::config::{
        DatabaseConfig, MuipConfig, MuipGmConfig, PathConfig, ServerConfig, ServerSettings,
    };
    use reqwest::Client;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::{path::PathBuf, sync::Once};

    static INIT_CONFIG: Once = Once::new();

    fn init_test_config() {
        INIT_CONFIG.call_once(|| {
            common::init_config(ServerConfig {
                server: ServerSettings {
                    host: "0.0.0.0".into(),
                    dns: "reverse1999.yezimoan.xyz".into(),
                    http_port: 32019,
                    game_port: 32020,
                    skip_tutorial: false,
                },
                muip: MuipConfig::default(),
                muip_gm: MuipGmConfig::default(),
                paths: PathConfig {
                    excel_data: PathBuf::from("data/excel2json"),
                },
                database: DatabaseConfig {
                    path: PathBuf::from("data/sonetto.db"),
                },
            });
        });
    }

    #[tokio::test]
    async fn payment_list_disables_all_payment_methods_and_pages() {
        init_test_config();
        let request = serde_json::from_value(serde_json::json!({
            "deviceInfo": {
                "networkName": "test", "deviceId": "test", "cnadid": "", "oaId": "",
                "androidId": "", "imsi": "", "imei": "", "uuid": "test",
                "deviceName": "test", "deviceManufacturer": "test", "osType": 1,
                "osVersion": "test", "apiLevel": "test", "language": "zh-CN",
                "displayWidth": "1", "displayHeight": "1", "hardware": "test",
                "buildName": "test", "distinctId": "test", "anonymousId": "test"
            },
            "appPackageInfo": {
                "appPackageName": "test", "appVersion": 1, "appVersionName": "test",
                "gameId": 60001, "gameCode": "test", "gameName": "test",
                "channelId": "200", "subChannelId": "200", "appInstallTime": "1",
                "appUpdateTime": "1", "appSignature": "test", "sdkVersion": "test",
                "channelVersion": "test", "adFid": "", "gclid": "", "dataAppId": ""
            },
            "userId": "readonly-test",
            "language": "zh-CN"
        }))
        .unwrap();
        let db = SqlitePoolOptions::new()
            .connect_lazy("sqlite::memory:")
            .unwrap();

        let response = post(
            State(AppState {
                sdk: SdkState {
                    http_client: Client::new(),
                },
                db,
            }),
            Json(request),
        )
        .await
        .0;

        assert!(response.data.payments.is_empty());
        assert!(response.data.web_pre_pay_url.is_empty());
    }
}

use crate::AppState;
use crate::models::request::OrderReq;
use crate::models::response::{OrderRsp, OrderRspData};
use axum::extract::State;
use axum::response::Json;

pub async fn post(
    State(_): State<AppState>,
    axum::Json(req): axum::Json<OrderReq>,
) -> Json<OrderRsp> {
    tracing::info!(
        "Received order request: goods_id={}, game_order_id={}",
        req.goods_id,
        req.game_order_id
    );

    let response = OrderRsp {
        code: 403,
        msg: "payments disabled".to_string(),
        data: OrderRspData {
            order_id: String::new(),
            pay_notify_url: String::new(),
            ext_params: String::new(),
        },
    };

    tracing::info!("Rejecting order because payments are disabled");

    Json(response)
}

#[cfg(test)]
mod tests {
    use super::post;
    use crate::{AppState, SdkState};
    use axum::{Json, extract::State};
    use reqwest::Client;
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn order_creation_is_rejected_before_the_payment_flow_starts() {
        let request = serde_json::from_value(serde_json::json!({
            "deviceInfo": {
                "networkName": "test", "deviceId": "test", "cnadid": "", "oaId": "",
                "androidId": "", "imsi": "", "imei": "", "uuid": "test",
                "deviceName": "test", "deviceManufacturer": "test", "osType": 3,
                "osVersion": "Windows", "apiLevel": "", "language": "zh-CN",
                "displayWidth": "1920", "displayHeight": "1080", "hardware": "test",
                "buildName": "test", "distinctId": "test", "anonymousId": "test"
            },
            "appPackageInfo": {
                "appPackageName": "test", "appVersion": 30605, "appVersionName": "3.6.5",
                "gameId": 60001, "gameCode": "reverse1999", "gameName": "Reverse: 1999",
                "channelId": "200", "subChannelId": "200", "appInstallTime": "0",
                "appUpdateTime": "0", "appSignature": "", "sdkVersion": "",
                "channelVersion": "", "adFid": "", "gclid": "", "dataAppId": ""
            },
            "userId": "1", "token": "test", "roleId": "1", "roleName": "test",
            "currentLevel": 1, "roleVipLvl": 0, "serverId": "1", "serverName": "test",
            "roleEstablishTime": 0, "roleType": "test", "giveCurrencyNum": 0,
            "paidCurrencyNum": 0, "currencyNum": 0, "amount": 1, "originAmount": 1,
            "originCurrency": "USD", "goodsId": "test", "currency": "USD",
            "goodsName": "test", "goodsDesc": "test", "gameOrderId": "test",
            "passBackParam": "", "notifyUrl": "", "timestamp": "0", "sign": "",
            "productId": "test", "currentProgress": "test"
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

        assert_eq!(response.code, 403);
        assert_eq!(response.msg, "payments disabled");
        assert!(response.data.order_id.is_empty());
        assert!(response.data.pay_notify_url.is_empty());
        assert!(response.data.ext_params.is_empty());
    }
}

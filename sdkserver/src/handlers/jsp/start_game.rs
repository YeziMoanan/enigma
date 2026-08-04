use crate::models::response::JspStartGameRsp;
use axum::response::Json;
use common::{dns, game_port};

pub async fn get() -> Json<JspStartGameRsp> {
    Json(build_response(dns(), game_port()))
}

fn build_response(public_host: &str, public_port: u16) -> JspStartGameRsp {
    JspStartGameRsp {
        bak_ip: String::from(public_host),
        bak_port: public_port,
        ip: String::from(public_host),
        port: public_port,
        state: 1,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::build_response;

    #[test]
    fn advertises_public_dns_instead_of_the_bind_host() {
        let rsp = build_response("reverse1999.yezimoan.xyz", 32020);

        assert_eq!(rsp.ip, "reverse1999.yezimoan.xyz");
        assert_eq!(rsp.bak_ip, "reverse1999.yezimoan.xyz");
        assert_eq!(rsp.port, 32020);
        assert_eq!(rsp.bak_port, 32020);
    }
}

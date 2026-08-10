use sonettobuf::Mail;
use sqlx::FromRow;

pub const CLIENT_LANGUAGE_SHORTCUTS: [&str; 8] = ["zh", "tw", "en", "kr", "jp", "de", "fr", "thai"];

pub fn localized_text(value: &str) -> String {
    serde_json::json!({
        "zh": value,
        "tw": value,
        "en": value,
        "kr": value,
        "jp": value,
        "de": value,
        "fr": value,
        "thai": value,
    })
    .to_string()
}

#[derive(Debug, Clone, FromRow)]
pub struct UserMail {
    pub incr_id: i64,
    pub user_id: i64,
    pub mail_id: i32,
    pub params: String,
    pub attachment: String,
    pub state: i32,
    pub create_time: i64,
    pub sender: String,
    pub title: String,
    pub content: String,
    pub copy: String,
    pub expire_time: i64,
    pub sender_type: i32,
    pub jump_title: String,
    pub jump: String,
    pub is_lock: bool,
}

impl From<UserMail> for Mail {
    fn from(mail: UserMail) -> Self {
        Mail {
            incr_id: Some(mail.incr_id as u64),
            mail_id: Some(mail.mail_id as u32),
            params: Some(mail.params),
            attachment: Some(mail.attachment),
            state: Some(mail.state as u32),
            create_time: Some(mail.create_time as u64),
            sender: Some(mail.sender),
            title: Some(mail.title),
            content: Some(mail.content),
            copy: Some(mail.copy),
            expire_time: Some(mail.expire_time as u64),
            sender_type: Some(mail.sender_type),
            jump_title: Some(mail.jump_title),
            jump: Some(mail.jump),
            is_lock: Some(mail.is_lock),
        }
    }
}

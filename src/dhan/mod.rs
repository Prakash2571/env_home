pub mod api;
pub mod callback;
pub mod sidecar;

pub fn is_valid_token(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_graphic())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Session {
    pub client_id: String,
    pub access_token: String,
    pub expires_at: i64,
    pub obtained_at: i64,
}

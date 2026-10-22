pub mod api;
pub mod callback;
pub mod sidecar;

pub fn is_valid_token(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_graphic())
}

#[derive(Clone, Eq, PartialEq)]
pub struct Session {
    pub client_id: String,
    pub access_token: String,
    pub expires_at: i64,
    pub obtained_at: i64,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Session")
            .field("client_id", &crate::state::client_id_hint(&self.client_id))
            .field("expires_at", &self.expires_at)
            .field("obtained_at", &self.obtained_at)
            .finish_non_exhaustive()
    }
}

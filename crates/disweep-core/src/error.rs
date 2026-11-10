#[derive(Clone, Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Couldn't reach Discord: {0}")]
    Network(String),

    #[error("Discord rejected this login. Sign in again.")]
    Unauthorized,

    #[error("Discord wants this account verified first. Open Discord and complete the check.")]
    VerificationRequired,

    #[error("Discord kept rate limiting this request. Try again later.")]
    Throttled,

    #[error(
        "Stopped to protect your connection: too many requests were refused in the last 10 minutes."
    )]
    InvalidRequestBudget,

    #[error("Discord's search isn't ready for this conversation yet. Try again in a few minutes.")]
    IndexUnavailable,

    #[error("{message} (HTTP {status}{})", code.map_or_else(String::new, |code| format!(", code {code}")))]
    Discord {
        status: u16,
        code: Option<u32>,
        message: String,
    },

    #[error("Discord sent a response Disweep couldn't read.")]
    Decode(String),
}

impl ApiError {
    #[must_use]
    pub fn code(&self) -> Option<u32> {
        match self {
            Self::Discord { code, .. } => *code,
            _ => None,
        }
    }

    #[must_use]
    pub fn is_account_problem(&self) -> bool {
        matches!(self, Self::Unauthorized | Self::VerificationRequired)
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

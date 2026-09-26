use thiserror::Error;

#[derive(Debug, Error)]
pub enum SourceError {
    #[error("ESPN rejected the session. Refresh ESPN_S2 and retry.")]
    EspnAuthenticationRequired,

    #[error("the provider rejected the request: {0}")]
    Request(#[from] reqwest::Error),

    #[error("the provider returned an invalid response: {0}")]
    InvalidResponse(String),
}

#[derive(Debug, thiserror::Error)]
pub enum SerdeError {
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Yaml(#[from] serde_saphyr::Error),
}

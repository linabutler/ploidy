#[derive(Debug, thiserror::Error)]
pub enum SerdeError {
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    JsonWithPath(#[from] serde_path_to_error::Error<serde_json::Error>),
    #[error(transparent)]
    Yaml(#[from] serde_yaml::Error),
    #[error(transparent)]
    YamlWithPath(#[from] serde_path_to_error::Error<serde_yaml::Error>),
}

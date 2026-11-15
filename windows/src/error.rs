#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppError {
    pub key: &'static str,
    pub detail: String,
}
impl AppError {
    pub fn new(key: &'static str) -> Self {
        Self {
            key,
            detail: String::new(),
        }
    }
    pub fn detail(key: &'static str, detail: impl ToString) -> Self {
        Self {
            key,
            detail: detail.to_string(),
        }
    }
}
impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.key, self.detail)
    }
}
impl std::error::Error for AppError {}
pub type Result<T> = std::result::Result<T, AppError>;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum HarblyError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("数据库错误: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("废纸篓错误: {0}")]
    Trash(#[from] trash::Error),
    #[error("{0}")]
    Msg(String),
}

impl HarblyError {
    pub fn msg(s: impl Into<String>) -> Self {
        HarblyError::Msg(s.into())
    }
}

pub type Result<T> = std::result::Result<T, HarblyError>;

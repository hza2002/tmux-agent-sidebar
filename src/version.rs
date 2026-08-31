#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateNotice {
    pub local_version: String,
    pub latest_version: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WordTokenCommandDto {
    pub start: f64,
    pub end: f64,
    pub word: String,
}

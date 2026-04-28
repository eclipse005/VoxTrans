#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TerminologyTermCommand {
    pub id: String,
    pub origin: String,
    pub target: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TerminologyGroupCommand {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub terms: Vec<TerminologyTermCommand>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleLineStyleCommand {
    pub font_family: String,
    pub font_size: u32,
    pub primary_color: String,
    pub outline_color: String,
    pub back_color: String,
    pub outline: f64,
    pub shadow: f64,
    pub border_style: String,
    pub border_opacity: u8,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleLayoutStyleCommand {
    pub margin_v: u32,
    pub alignment: u8,
    pub bilingual_line_gap: u32,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleRenderStyleCommand {
    pub source: SubtitleLineStyleCommand,
    pub target: SubtitleLineStyleCommand,
    pub layout: SubtitleLayoutStyleCommand,
}

impl Default for SubtitleRenderStyleCommand {
    fn default() -> Self {
        Self {
            source: SubtitleLineStyleCommand {
                font_family: "Arial".to_string(),
                font_size: 44,
                primary_color: "#FFFFFF".to_string(),
                outline_color: "#101010".to_string(),
                back_color: "#000000".to_string(),
                outline: 2.5,
                shadow: 1.0,
                border_style: "outline".to_string(),
                border_opacity: 88,
            },
            target: SubtitleLineStyleCommand {
                font_family: "Microsoft YaHei".to_string(),
                font_size: 40,
                primary_color: "#EAF6FF".to_string(),
                outline_color: "#101010".to_string(),
                back_color: "#000000".to_string(),
                outline: 2.5,
                shadow: 1.0,
                border_style: "outline".to_string(),
                border_opacity: 88,
            },
            layout: SubtitleLayoutStyleCommand {
                margin_v: 40,
                alignment: 2,
                bilingual_line_gap: 10,
            },
        }
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSettingsCommand {
    pub provider: String,
    pub chunk_target_seconds: u32,
    pub subtitle_max_words_per_segment: u32,
    pub subtitle_length_reference: u32,
    pub asr_model: String,
    pub demucs_model: String,
    pub enable_vocal_separation: bool,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    pub llm_concurrency: u32,
    #[serde(default)]
    pub terminology_groups: Vec<TerminologyGroupCommand>,
    #[serde(default = "default_true")]
    pub enable_terminology: bool,
    #[serde(default = "default_true")]
    pub enable_subtitle_beautify: bool,
    #[serde(default)]
    pub auto_burn_hard_subtitle: bool,
    #[serde(default = "default_subtitle_burn_mode")]
    pub subtitle_burn_mode: String,
    #[serde(default)]
    pub subtitle_render_style: SubtitleRenderStyleCommand,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPreferencesCommandResponse {
    pub settings: SavedSettingsCommand,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAppSettingsCommandRequest {
    pub settings: SavedSettingsCommand,
}

fn default_true() -> bool {
    true
}

fn default_subtitle_burn_mode() -> String {
    "bilingualSourceFirst".to_string()
}

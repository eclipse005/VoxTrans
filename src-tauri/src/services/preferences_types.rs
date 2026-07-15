use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename_all = "camelCase")]
pub enum SubtitleBorderStyle {
    #[default]
    Outline,
    Box,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Cpu,
    Cuda,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Cuda => "cuda",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim() {
            "cuda" => Self::Cuda,
            _ => Self::Cpu,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, rename_all = "lowercase")]
pub enum SubtitleLengthPreset {
    Short,
    #[default]
    Standard,
    Loose,
}

impl SubtitleLengthPreset {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Short => "short",
            Self::Standard => "standard",
            Self::Loose => "loose",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim() {
            "short" => Self::Short,
            "loose" => Self::Loose,
            _ => Self::Standard,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, TS)]
pub enum AsrModel {
    #[default]
    #[serde(rename = "Qwen3-ASR-0.6B")]
    Qwen3Asr06B,
    #[serde(rename = "Qwen3-ASR-1.7B")]
    Qwen3Asr17B,
    #[serde(rename = "cohere-transcribe-03-2026")]
    CohereTranscribe032026,
    #[serde(rename = "moss-transcribe-diarize")]
    MossTranscribeDiarize,
}

impl fmt::Display for AsrModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl AsrModel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Qwen3Asr06B => "Qwen3-ASR-0.6B",
            Self::Qwen3Asr17B => "Qwen3-ASR-1.7B",
            Self::CohereTranscribe032026 => "cohere-transcribe-03-2026",
            Self::MossTranscribeDiarize => "moss-transcribe-diarize",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim() {
            "Qwen3-ASR-1.7B" => Self::Qwen3Asr17B,
            "cohere-transcribe-03-2026" => Self::CohereTranscribe032026,
            "moss-transcribe-diarize" => Self::MossTranscribeDiarize,
            _ => Self::Qwen3Asr06B,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, TS)]
pub enum AlignModel {
    #[default]
    #[serde(rename = "Qwen3-ForcedAligner-0.6B")]
    Qwen3ForcedAligner06B,
}

impl fmt::Display for AlignModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl AlignModel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Qwen3ForcedAligner06B => "Qwen3-ForcedAligner-0.6B",
        }
    }

    pub fn parse(value: &str) -> Self {
        let _ = value.trim();
        Self::Qwen3ForcedAligner06B
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, TS)]
pub enum DemucsModel {
    #[default]
    #[serde(rename = "htdemucs_ft")]
    HtdemucsFt,
}

impl DemucsModel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HtdemucsFt => "htdemucs_ft",
        }
    }

    pub fn parse(value: &str) -> Self {
        let _ = value.trim();
        Self::HtdemucsFt
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize, Default, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename_all = "camelCase")]
pub enum SubtitleBurnMode {
    Source,
    Target,
    #[default]
    BilingualSourceFirst,
    BilingualTargetFirst,
}

impl SubtitleBurnMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Target => "target",
            Self::BilingualSourceFirst => "bilingualSourceFirst",
            Self::BilingualTargetFirst => "bilingualTargetFirst",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim() {
            "source" => Self::Source,
            "target" => Self::Target,
            "bilingualTargetFirst" => Self::BilingualTargetFirst,
            _ => Self::BilingualSourceFirst,
        }
    }
}

impl SubtitleBorderStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Outline => "outline",
            Self::Box => "box",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim() {
            "box" => Self::Box,
            _ => Self::Outline,
        }
    }
}

/// Lenient `Deserialize`: a malformed or unrecognized value in the persisted
/// `subtitle_render_style_json` falls back to the default (Outline) instead
/// of failing the entire `load_settings` read. Mirrors the legacy
/// `normalize_border_style()` behavior that mapped any unknown value to
/// "outline". Implemented on the type (not via field-level
/// `deserialize_with`) so ts-rs can still parse the struct's serde attrs.
impl<'de> Deserialize<'de> for SubtitleBorderStyle {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(s) => Ok(Self::parse(&s)),
            // null / numbers / objects / anything unexpected → default.
            _ => Ok(Self::Outline),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename_all = "camelCase")]
pub struct TerminologyTerm {
    pub id: String,
    pub origin: String,
    pub target: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename_all = "camelCase")]
pub struct TerminologyGroup {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub terms: Vec<TerminologyTerm>,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename_all = "camelCase")]
pub struct SubtitleLineStyle {
    pub font_family: String,
    pub font_size: u32,
    pub primary_color: String,
    pub outline_color: String,
    pub back_color: String,
    pub outline: f64,
    pub shadow: f64,
    #[serde(default)]
    pub border_style: SubtitleBorderStyle,
    pub border_opacity: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename_all = "camelCase")]
pub struct SubtitleLayoutStyle {
    pub margin_v: u32,
    pub alignment: u8,
    pub bilingual_line_gap: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename_all = "camelCase")]
pub struct SubtitleRenderStyle {
    pub source: SubtitleLineStyle,
    pub target: SubtitleLineStyle,
    pub layout: SubtitleLayoutStyle,
}

impl Default for SubtitleRenderStyle {
    fn default() -> Self {
        Self {
            source: SubtitleLineStyle {
                font_family: "Arial".to_string(),
                font_size: 44,
                primary_color: "#FFFFFF".to_string(),
                outline_color: "#101010".to_string(),
                back_color: "#000000".to_string(),
                outline: 2.5,
                shadow: 1.0,
                border_style: SubtitleBorderStyle::Outline,
                border_opacity: 88,
            },
            target: SubtitleLineStyle {
                font_family: "Microsoft YaHei".to_string(),
                font_size: 40,
                primary_color: "#EAF6FF".to_string(),
                outline_color: "#101010".to_string(),
                back_color: "#000000".to_string(),
                outline: 2.5,
                shadow: 1.0,
                border_style: SubtitleBorderStyle::Outline,
                border_opacity: 88,
            },
            layout: SubtitleLayoutStyle {
                margin_v: 40,
                alignment: 2,
                bilingual_line_gap: 10,
            },
        }
    }
}

/// One LLM provider slot (Egg-style multi-profile). `id` matches a preset id
/// (`deepseek`, `custom`, …). Pipeline still reads the denormalized
/// `translate_*` fields, which always mirror the active profile after normalize.
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename_all = "camelCase")]
pub struct LlmProfile {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    /// Which vendor preset this slot came from (usually equals `id`).
    #[serde(default)]
    pub preset_id: String,
    /// When false (e.g. local Ollama), empty key is allowed and treated as `"ollama"`.
    #[serde(default = "default_true")]
    pub requires_key: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename_all = "camelCase")]
pub struct SavedSettings {
    pub provider: Provider,
    pub chunk_target_seconds: u32,
    pub subtitle_length_preset: SubtitleLengthPreset,
    pub asr_model: AsrModel,
    #[serde(default = "default_align_model")]
    pub align_model: AlignModel,
    pub demucs_model: DemucsModel,
    pub enable_vocal_separation: bool,
    pub translate_api_key: String,
    pub translate_base_url: String,
    pub translate_model: String,
    /// Multi-provider LLM archives. Source of truth for Key/URL/Model per vendor.
    #[serde(default)]
    pub llm_profiles: Vec<LlmProfile>,
    /// Active profile id within `llm_profiles` (e.g. `"deepseek"`).
    #[serde(default)]
    pub active_llm_profile_id: String,
    pub llm_concurrency: u32,
    #[serde(default)]
    pub terminology_groups: Vec<TerminologyGroup>,
    #[serde(default)]
    pub active_terminology_group_id: String,
    #[serde(default = "default_true")]
    pub enable_subtitle_beautify: bool,
    #[serde(default = "default_true")]
    pub enable_click_sound: bool,
    #[serde(default)]
    pub auto_burn_hard_subtitle: bool,
    #[serde(default = "default_subtitle_burn_mode")]
    pub subtitle_burn_mode: SubtitleBurnMode,
    #[serde(default)]
    pub subtitle_render_style: SubtitleRenderStyle,
    #[serde(default)]
    pub flat_srt_output: bool,
    #[serde(default = "default_flat_srt_items")]
    pub flat_srt_items: Vec<SubtitleBurnMode>,
    #[serde(default)]
    pub enable_vision_assist: bool,
    #[serde(default = "default_locale")]
    pub locale: Locale,
    /// Custom model storage directory. When `None` or empty, models are stored
    /// under the executable's `models/` subdirectory.
    #[serde(default)]
    pub models_dir: Option<String>,
}

/// UI locale. Defaults to Simplified Chinese (the app's original language).
///
/// The TS/serde representation uses canonical BCP-47 tags (`"zh-CN"` / `"en"`)
/// so the value round-trips unchanged across the DB (`as_str`/`parse`), Tauri
/// IPC (serde), and the frontend i18next resources (`AppLocale`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, TS)]
#[ts(export)]
pub enum Locale {
    #[default]
    #[serde(rename = "zh-CN")]
    #[ts(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "en")]
    #[ts(rename = "en")]
    En,
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Locale {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN",
            Self::En => "en",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim() {
            "en" => Self::En,
            _ => Self::ZhCn,
        }
    }
}

fn default_locale() -> Locale {
    Locale::default()
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename_all = "camelCase")]
pub struct UserPreferencesResponse {
    pub settings: SavedSettings,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename_all = "camelCase")]
pub struct SaveAppSettingsRequest {
    pub settings: SavedSettings,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename_all = "camelCase")]
pub struct DefaultSettingsResponse {
    pub settings: SavedSettings,
}

fn default_true() -> bool {
    true
}

fn default_align_model() -> AlignModel {
    AlignModel::Qwen3ForcedAligner06B
}

fn default_subtitle_burn_mode() -> SubtitleBurnMode {
    SubtitleBurnMode::BilingualSourceFirst
}

fn default_flat_srt_items() -> Vec<SubtitleBurnMode> {
    vec![SubtitleBurnMode::Source, SubtitleBurnMode::Target]
}

#[cfg(test)]
mod ts_export_tests {
    use super::*;

    #[test]
    fn export_preference_types() {
        Provider::export_all().expect("export Provider");
        SubtitleLengthPreset::export_all().expect("export SubtitleLengthPreset");
        AsrModel::export_all().expect("export AsrModel");
        AlignModel::export_all().expect("export AlignModel");
        DemucsModel::export_all().expect("export DemucsModel");
        SubtitleBurnMode::export_all().expect("export SubtitleBurnMode");
        SubtitleBorderStyle::export_all().expect("export SubtitleBorderStyle");
        TerminologyTerm::export_all().expect("export TerminologyTerm");
        TerminologyGroup::export_all().expect("export TerminologyGroup");
        SubtitleLineStyle::export_all().expect("export SubtitleLineStyle");
        SubtitleLayoutStyle::export_all().expect("export SubtitleLayoutStyle");
        SubtitleRenderStyle::export_all().expect("export SubtitleRenderStyle");
        Locale::export_all().expect("export Locale");
        LlmProfile::export_all().expect("export LlmProfile");
        SavedSettings::export_all().expect("export SavedSettings");
        UserPreferencesResponse::export_all().expect("export UserPreferencesResponse");
        SaveAppSettingsRequest::export_all().expect("export SaveAppSettingsRequest");
        DefaultSettingsResponse::export_all().expect("export DefaultSettingsResponse");
    }
}

#[cfg(test)]
mod enum_parse_tests {
    use super::*;

    #[test]
    fn provider_parse_handles_valid_and_invalid() {
        assert_eq!(Provider::parse("cpu"), Provider::Cpu);
        assert_eq!(Provider::parse("cuda"), Provider::Cuda);
        assert_eq!(Provider::parse("  cuda  "), Provider::Cuda);
        // Uppercase/non-matching values fall back to default (parse is
        // case-sensitive, matching legacy backend behavior).
        assert_eq!(Provider::parse("CUDA"), Provider::Cpu);
        assert_eq!(Provider::parse("openai"), Provider::Cpu);
        assert_eq!(Provider::parse(""), Provider::Cpu);
    }

    #[test]
    fn subtitle_length_preset_parse_maps_legacy_default_to_standard() {
        // "default" was a legacy value; must map to Standard (the new default).
        assert_eq!(SubtitleLengthPreset::parse("default"), SubtitleLengthPreset::Standard);
        assert_eq!(SubtitleLengthPreset::parse("short"), SubtitleLengthPreset::Short);
        assert_eq!(SubtitleLengthPreset::parse("loose"), SubtitleLengthPreset::Loose);
        assert_eq!(SubtitleLengthPreset::parse("standard"), SubtitleLengthPreset::Standard);
        assert_eq!(SubtitleLengthPreset::parse("unknown"), SubtitleLengthPreset::Standard);
        assert_eq!(SubtitleLengthPreset::parse(""), SubtitleLengthPreset::Standard);
    }

    #[test]
    fn asr_model_parse_maps_empty_to_default() {
        assert_eq!(AsrModel::parse(""), AsrModel::Qwen3Asr06B);
        assert_eq!(AsrModel::parse("unknown"), AsrModel::Qwen3Asr06B);
        assert_eq!(
            AsrModel::parse("Qwen3-ASR-1.7B"),
            AsrModel::Qwen3Asr17B
        );
        assert_eq!(
            AsrModel::parse("cohere-transcribe-03-2026"),
            AsrModel::CohereTranscribe032026
        );
        assert_eq!(
            AsrModel::parse("moss-transcribe-diarize"),
            AsrModel::MossTranscribeDiarize
        );
    }

    #[test]
    fn subtitle_burn_mode_parse_maps_invalid_to_default() {
        assert_eq!(
            SubtitleBurnMode::parse("source"),
            SubtitleBurnMode::Source
        );
        assert_eq!(
            SubtitleBurnMode::parse("bilingualTargetFirst"),
            SubtitleBurnMode::BilingualTargetFirst
        );
        assert_eq!(
            SubtitleBurnMode::parse("invalid"),
            SubtitleBurnMode::BilingualSourceFirst
        );
    }

    #[test]
    fn enum_serde_roundtrips_keep_db_values_stable() {
        // Verify serde rename keeps DB-stored values stable across refactor.
        assert_eq!(serde_json::to_string(&Provider::Cpu).unwrap(), "\"cpu\"");
        assert_eq!(serde_json::to_string(&Provider::Cuda).unwrap(), "\"cuda\"");
        assert_eq!(
            serde_json::to_string(&SubtitleLengthPreset::Short).unwrap(),
            "\"short\""
        );
        assert_eq!(
            serde_json::to_string(&SubtitleLengthPreset::Standard).unwrap(),
            "\"standard\""
        );
        assert_eq!(
            serde_json::to_string(&SubtitleLengthPreset::Loose).unwrap(),
            "\"loose\""
        );
        assert_eq!(
            serde_json::to_string(&AsrModel::Qwen3Asr06B).unwrap(),
            "\"Qwen3-ASR-0.6B\""
        );
        assert_eq!(
            serde_json::to_string(&AsrModel::Qwen3Asr17B).unwrap(),
            "\"Qwen3-ASR-1.7B\""
        );
        assert_eq!(
            serde_json::to_string(&AsrModel::CohereTranscribe032026).unwrap(),
            "\"cohere-transcribe-03-2026\""
        );
        assert_eq!(
            serde_json::to_string(&AsrModel::MossTranscribeDiarize).unwrap(),
            "\"moss-transcribe-diarize\""
        );
        assert_eq!(
            serde_json::to_string(&AlignModel::Qwen3ForcedAligner06B).unwrap(),
            "\"Qwen3-ForcedAligner-0.6B\""
        );
        assert_eq!(
            serde_json::to_string(&DemucsModel::HtdemucsFt).unwrap(),
            "\"htdemucs_ft\""
        );
        assert_eq!(
            serde_json::to_string(&SubtitleBurnMode::Source).unwrap(),
            "\"source\""
        );
        assert_eq!(
            serde_json::to_string(&SubtitleBurnMode::BilingualSourceFirst).unwrap(),
            "\"bilingualSourceFirst\""
        );
        assert_eq!(
            serde_json::to_string(&SubtitleBorderStyle::Outline).unwrap(),
            "\"outline\""
        );
        assert_eq!(
            serde_json::to_string(&SubtitleBorderStyle::Box).unwrap(),
            "\"box\""
        );

        // Round-trip: deserialize must accept these exact values.
        let cpu: Provider = serde_json::from_str("\"cpu\"").unwrap();
        assert_eq!(cpu, Provider::Cpu);
        let loose: SubtitleLengthPreset = serde_json::from_str("\"loose\"").unwrap();
        assert_eq!(loose, SubtitleLengthPreset::Loose);
        let asr: AsrModel = serde_json::from_str("\"Qwen3-ASR-1.7B\"").unwrap();
        assert_eq!(asr, AsrModel::Qwen3Asr17B);
        let burn: SubtitleBurnMode = serde_json::from_str("\"bilingualTargetFirst\"").unwrap();
        assert_eq!(burn, SubtitleBurnMode::BilingualTargetFirst);
    }
}


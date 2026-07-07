#[derive(Debug, Clone, Copy)]
pub enum TaskStage {
    Preparing,
    Separating,
    Recognizing,
    Aligning,
    Segmenting,
    Terminology,
    Translating,
    SubtitleLayout,
    Burning,
}

impl TaskStage {
    pub fn code(self) -> &'static str {
        match self {
            TaskStage::Preparing => "preparing",
            TaskStage::Separating => "separating",
            TaskStage::Recognizing => "recognizing",
            TaskStage::Aligning => "aligning",
            TaskStage::Segmenting => "segmenting",
            TaskStage::Terminology => "terminology",
            TaskStage::Translating => "translating",
            TaskStage::SubtitleLayout => "subtitleLayout",
            TaskStage::Burning => "burning",
        }
    }

    /// Human-readable label. Returns empty for all stages: the frontend owns
    /// the localized labels and derives them from `code()` via its i18n layer
    /// (`stage.label || resolveStageLabel(stage.code)`). Keeping the backend
    /// label empty avoids shipping localized text from the backend.
    pub fn label(self) -> &'static str {
        ""
    }

    pub fn order(self) -> u32 {
        match self {
            TaskStage::Preparing => 20,
            TaskStage::Separating => 25,
            TaskStage::Recognizing => 30,
            TaskStage::Aligning => 35,
            TaskStage::Segmenting => 40,
            TaskStage::Terminology => 60,
            TaskStage::Translating => 70,
            TaskStage::SubtitleLayout => 80,
            TaskStage::Burning => 95,
        }
    }
}

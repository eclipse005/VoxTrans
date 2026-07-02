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

    pub fn label(self) -> &'static str {
        match self {
            TaskStage::Preparing => "准备中",
            TaskStage::Separating => "人声分离中",
            TaskStage::Recognizing => "语音识别中",
            TaskStage::Aligning => "智能打轴中",
            TaskStage::Segmenting => "断句中",
            TaskStage::Terminology => "术语提取中",
            TaskStage::Translating => "翻译中",
            TaskStage::SubtitleLayout => "",
            TaskStage::Burning => "压制中",
        }
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

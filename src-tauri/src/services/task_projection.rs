use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskProjectionState {
    pub queue: TaskProjectionQueueState,
    pub editor: TaskProjectionEditorState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskProjectionQueueState {
    pub transcribe_status: String,
    pub phase: String,
    #[serde(default)]
    pub phase_detail: String,
    pub progress_percent: u32,
    pub transcribe_segment_current: u32,
    pub transcribe_segment_total: u32,
    pub transcribe_error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskProjectionEditorState {
    pub subtitle_segments_json: String,
    pub result_text: String,
    pub result_srt: String,
    pub translated_srt: String,
}

impl TaskProjectionState {
    pub fn new() -> Self {
        Self {
            queue: TaskProjectionQueueState {
                transcribe_status: "queued".to_string(),
                phase: String::new(),
                phase_detail: String::new(),
                progress_percent: 0,
                transcribe_segment_current: 0,
                transcribe_segment_total: 0,
                transcribe_error: String::new(),
            },
            editor: TaskProjectionEditorState {
                subtitle_segments_json: "[]".to_string(),
                result_text: String::new(),
                result_srt: String::new(),
                translated_srt: String::new(),
            },
        }
    }

    pub fn set_queue(
        &mut self,
        status: &str,
        phase: &str,
        phase_detail: &str,
        progress_percent: u32,
        current: u32,
        total: u32,
        error: &str,
    ) -> u32 {
        self.queue = TaskProjectionQueueState {
            transcribe_status: status.to_string(),
            phase: phase.to_string(),
            phase_detail: phase_detail.to_string(),
            progress_percent,
            transcribe_segment_current: current,
            transcribe_segment_total: total,
            transcribe_error: error.to_string(),
        };
        progress_percent.clamp(0, 100)
    }

    pub fn set_editor(
        &mut self,
        subtitle_segments_json: String,
        result_text: String,
        result_srt: String,
        translated_srt: String,
    ) {
        self.editor = TaskProjectionEditorState {
            subtitle_segments_json,
            result_text,
            result_srt,
            translated_srt,
        };
    }
}

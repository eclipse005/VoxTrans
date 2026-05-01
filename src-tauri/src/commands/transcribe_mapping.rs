use super::transcribe_types::{TranscribeCommandResponse, WordTokenCommandDto};

pub(super) fn from_transcribe_response(
    response: crate::services::transcribe::TranscribeResponse,
) -> TranscribeCommandResponse {
    TranscribeCommandResponse {
        words: response.words.into_iter().map(from_service_word).collect(),
        text: response.text,
        aligned_text: response.aligned_text,
        segment_total: response.segment_total,
        segment_durations_sec: response.segment_durations_sec,
        audio_duration_sec: response.audio_duration_sec,
        vad_elapsed_sec: response.vad_elapsed_sec,
        transcribe_elapsed_sec: response.transcribe_elapsed_sec,
        timing_sec: response.timing_sec,
        rtf_x: response.rtf_x,
        rtf_breakdown_x: response.rtf_breakdown_x,
        execution_provider: response.execution_provider,
    }
}

fn from_service_word(word: crate::services::transcribe::WordTokenDto) -> WordTokenCommandDto {
    WordTokenCommandDto {
        start: word.start,
        end: word.end,
        word: word.word,
    }
}

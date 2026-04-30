use super::transcribe_types::{
    BuildSegmentsCommandResponse, SegmentWithWordsCommandDto, TranscribeCommandResponse,
    WordTokenCommandDto,
};

pub(super) fn to_service_word(
    word: WordTokenCommandDto,
) -> crate::services::transcribe::WordTokenDto {
    crate::services::transcribe::WordTokenDto {
        start: word.start,
        end: word.end,
        word: word.word,
    }
}

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

pub(super) fn from_build_segments_response(
    response: crate::services::transcribe::BuildSegmentsResponse,
) -> BuildSegmentsCommandResponse {
    BuildSegmentsCommandResponse {
        text: response.text,
        srt: response.srt,
        srt_output_path: response.srt_output_path,
        segments: response
            .segments
            .into_iter()
            .map(from_service_segment)
            .collect(),
    }
}

fn from_service_segment(
    segment: crate::services::transcribe::SegmentWithWordsDto,
) -> SegmentWithWordsCommandDto {
    SegmentWithWordsCommandDto {
        start: segment.start,
        end: segment.end,
        text: segment.text,
        words: segment.words.into_iter().map(from_service_word).collect(),
    }
}

fn from_service_word(word: crate::services::transcribe::WordTokenDto) -> WordTokenCommandDto {
    WordTokenCommandDto {
        start: word.start,
        end: word.end,
        word: word.word,
    }
}

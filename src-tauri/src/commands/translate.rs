#[cfg(test)]
use super::translate_artifacts::{
    default_task_id_from_path, parse_step2_segments_artifact_for_input,
};
#[cfg(test)]
use super::translate_terms::{count_source_tokens, normalize_command_terminology_entries};

pub use super::translate_types::*;

pub(crate) fn is_tail_ellipsis(text: &str) -> bool {
    let trimmed = text.trim_end();
    trimmed.ends_with("...") || trimmed.ends_with('…')
}

#[cfg(test)]
mod tests {
    use super::{
        TranslateTerminologyEntryCommand, count_source_tokens, default_task_id_from_path,
        normalize_command_terminology_entries, parse_step2_segments_artifact_for_input,
    };

    #[test]
    fn parse_step2_segments_accepts_flat_array_shape() {
        let raw = r#"
        [
          {
            "segment": "Hello world",
            "start": 0.0,
            "end": 1.2,
            "tokens": [
              { "text": "Hello", "start": 0.0, "end": 0.5 },
              { "text": "world", "start": 0.5, "end": 1.2 }
            ]
          }
        ]
        "#;
        let segments = parse_step2_segments_artifact_for_input(raw).expect("parse");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].segment, "Hello world");
        assert_eq!(segments[0].tokens.len(), 2);
        assert_eq!(segments[0].tokens[0].text, "Hello");
    }

    #[test]
    fn parse_step2_segments_accepts_wrapped_shape() {
        let raw = r#"
        {
          "taskId": "task-1",
          "mediaPath": "demo.mp4",
          "segments": [
            {
              "segment": "你好世界",
              "start": 0.0,
              "end": 1.0,
              "tokens": [
                { "text": "你", "start": 0.0, "end": 0.2 },
                { "text": "好", "start": 0.2, "end": 0.4 }
              ]
            }
          ]
        }
        "#;
        let segments = parse_step2_segments_artifact_for_input(raw).expect("parse");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].segment, "你好世界");
        assert_eq!(segments[0].tokens.len(), 2);
    }

    #[test]
    fn normalize_terminology_deduplicates_and_trims() {
        let normalized = normalize_command_terminology_entries(vec![
            TranslateTerminologyEntryCommand {
                source: "  NATO ".to_string(),
                target: "北约".to_string(),
                note: "a".to_string(),
            },
            TranslateTerminologyEntryCommand {
                source: "nato".to_string(),
                target: " 北约 ".to_string(),
                note: "b".to_string(),
            },
            TranslateTerminologyEntryCommand {
                source: "EU".to_string(),
                target: "欧盟".to_string(),
                note: " ".to_string(),
            },
        ]);

        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized[0].source, "NATO");
        assert_eq!(normalized[1].source, "EU");
    }

    #[test]
    fn source_token_count_prefers_tokens_and_falls_back_to_segment_text() {
        let raw = r#"
        [
          {
            "segment": "Hello world",
            "start": 0.0,
            "end": 1.2,
            "tokens": [
              { "text": "Hello", "start": 0.0, "end": 0.5 },
              { "text": "world", "start": 0.5, "end": 1.2 }
            ]
          },
          {
            "segment": "你好世界",
            "start": 1.2,
            "end": 2.0,
            "tokens": []
          }
        ]
        "#;
        let segments = parse_step2_segments_artifact_for_input(raw).expect("parse");
        assert_eq!(count_source_tokens(&segments), 3);
    }

    #[test]
    fn default_task_id_uses_file_stem() {
        let task_id = default_task_id_from_path(r"D:\output\step_02_segments.json");
        assert_eq!(task_id, "step_02_segments");
    }
}

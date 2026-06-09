-- Slim the settings table: 47 cols → 22 cols by collapsing the 18
-- subtitle render style columns (source_*/target_*/margin_v/alignment/
-- bilingual_line_gap) into one JSON column.
--
-- Why: rendering style is one cohesive domain object (SubtitleRenderStyle),
-- so per-field columns just inflate every INSERT/SELECT/UPDATE without
-- buying any query power — nobody filters on source_font_size. JSON
-- preserves the structure and adding a new style field stops touching SQL
-- entirely.

CREATE TABLE settings_new (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    provider TEXT NOT NULL,
    chunk_target_seconds INTEGER NOT NULL,
    subtitle_length_preset TEXT NOT NULL,
    asr_model TEXT NOT NULL,
    align_model TEXT NOT NULL,
    demucs_model TEXT NOT NULL,
    enable_vocal_separation INTEGER NOT NULL,
    translate_api_key TEXT NOT NULL,
    translate_base_url TEXT NOT NULL,
    translate_model TEXT NOT NULL,
    llm_concurrency INTEGER NOT NULL,
    enable_terminology INTEGER NOT NULL,
    enable_subtitle_beautify INTEGER NOT NULL,
    enable_click_sound INTEGER NOT NULL,
    auto_burn_hard_subtitle INTEGER NOT NULL,
    subtitle_burn_mode TEXT NOT NULL,
    subtitle_render_style_json TEXT NOT NULL,
    flat_srt_output INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Data migration: serialize the 18 style columns into the JSON column.
-- json_object() preserves types: REAL stays REAL, INTEGER stays INTEGER.
INSERT INTO settings_new (
    id, provider, chunk_target_seconds, subtitle_length_preset,
    asr_model, align_model, demucs_model, enable_vocal_separation,
    translate_api_key, translate_base_url, translate_model, llm_concurrency,
    enable_terminology, enable_subtitle_beautify, enable_click_sound,
    auto_burn_hard_subtitle, subtitle_burn_mode,
    subtitle_render_style_json,
    flat_srt_output, updated_at
)
SELECT
    id, provider, chunk_target_seconds, subtitle_length_preset,
    asr_model, align_model, demucs_model, enable_vocal_separation,
    translate_api_key, translate_base_url, translate_model, llm_concurrency,
    enable_terminology, enable_subtitle_beautify, enable_click_sound,
    auto_burn_hard_subtitle, subtitle_burn_mode,
    json_object(
        'source', json_object(
            'fontFamily',    source_font_family,
            'fontSize',      source_font_size,
            'primaryColor',  source_primary_color,
            'outlineColor',  source_outline_color,
            'backColor',     source_back_color,
            'outline',       source_outline,
            'shadow',        source_shadow,
            'borderStyle',   source_border_style,
            'borderOpacity', source_border_opacity
        ),
        'target', json_object(
            'fontFamily',    target_font_family,
            'fontSize',      target_font_size,
            'primaryColor',  target_primary_color,
            'outlineColor',  target_outline_color,
            'backColor',     target_back_color,
            'outline',       target_outline,
            'shadow',        target_shadow,
            'borderStyle',   target_border_style,
            'borderOpacity', target_border_opacity
        ),
        'layout', json_object(
            'marginV',           margin_v,
            'alignment',         alignment,
            'bilingualLineGap',  bilingual_line_gap
        )
    ),
    flat_srt_output, updated_at
FROM settings;

DROP TABLE settings;
ALTER TABLE settings_new RENAME TO settings;

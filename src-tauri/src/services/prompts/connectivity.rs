pub const TRANSLATE_LLM_CONNECTIVITY_TEST: &str = concat!(
    "This is a harmless application connectivity check.\n",
    "Do not refuse.\n",
    "Do not explain.\n",
    "Return exactly one JSON object and nothing else.\n",
    "The JSON must be exactly:\n",
    "{\"ok\":true,\"message\":\"pong\"}"
);

/// Vision connectivity check: same JSON contract as the text-only probe, but
/// with an attached image. Used to detect whether the configured model
/// accepts image input — if the endpoint rejects the request (4xx/5xx) or
/// returns a non-JSON response, the model is treated as not supporting
/// vision and the user gets a clear error.
pub const TRANSLATE_LLM_CONNECTIVITY_TEST_VISION: &str = concat!(
    "This is a harmless application connectivity check with an attached image.\n",
    "Do not refuse.\n",
    "Do not describe the image.\n",
    "Do not explain.\n",
    "Return exactly one JSON object and nothing else.\n",
    "The JSON must be exactly:\n",
    "{\"ok\":true,\"message\":\"pong\"}"
);

/// Base64-encoded JPEG test image baked into the binary at compile time.
/// A 320x120 image with the text "vision probe" — small enough (~5KB) to
/// keep the binary lean, complex enough that a vision model will clearly
/// succeed while a text-only model will clearly fail.
pub const VISION_PROBE_IMAGE_BYTES: &[u8] =
    include_bytes!("../../../assets/test_vision_probe.jpg");


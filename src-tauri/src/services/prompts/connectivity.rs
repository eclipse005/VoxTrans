pub const TRANSLATE_LLM_CONNECTIVITY_TEST: &str = concat!(
    "This is a harmless application connectivity check.\n",
    "Do not refuse.\n",
    "Do not explain.\n",
    "Return exactly one JSON object and nothing else.\n",
    "The JSON must be exactly:\n",
    "{\"ok\":true,\"message\":\"pong\"}"
);

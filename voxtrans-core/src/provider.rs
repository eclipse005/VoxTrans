use parakeet_rs::ExecutionProvider;

#[derive(Debug, Clone, Copy)]
pub enum Provider {
    Cpu,
    Directml,
}

impl Provider {
    pub fn from_id(raw: &str) -> Option<Self> {
        let normalized = raw.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "cpu" => Some(Self::Cpu),
            "directml" => Some(Self::Directml),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Directml => "directml",
        }
    }

    pub fn supported_ids() -> &'static [&'static str] {
        &["cpu", "directml"]
    }
}

pub(crate) fn to_execution_provider(provider: Provider) -> ExecutionProvider {
    match provider {
        Provider::Cpu => ExecutionProvider::Cpu,
        Provider::Directml => ExecutionProvider::DirectML,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Namespace {
    Value,
    Type,
    Macro,
}

impl std::fmt::Display for Namespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Namespace::Value => write!(f, "value"),
            Namespace::Type => write!(f, "type"),
            Namespace::Macro => write!(f, "macro"),
        }
    }
}

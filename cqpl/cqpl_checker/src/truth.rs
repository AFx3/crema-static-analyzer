use serde::Serialize;

/// Three-valued truth domain B = {ff < unk < tt} used by CQPL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Truth {
    False,
    Unknown,
    True,
}

impl Truth {
    pub fn not(self) -> Self {
        match self {
            Self::False => Self::True,
            Self::Unknown => Self::Unknown,
            Self::True => Self::False,
        }
    }

    /// Meet in ff < unk < tt.
    pub fn meet(self, other: Self) -> Self {
        std::cmp::min(self, other)
    }

    /// Join in ff < unk < tt.
    pub fn join(self, other: Self) -> Self {
        std::cmp::max(self, other)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::False => "ff",
            Self::Unknown => "unk",
            Self::True => "tt",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Truth::*;

    #[test]
    fn three_valued_lattice_and_negation_match_theory() {
        assert_eq!(False.join(Unknown), Unknown);
        assert_eq!(Unknown.join(True), True);
        assert_eq!(True.meet(Unknown), Unknown);
        assert_eq!(Unknown.meet(False), False);
        assert_eq!(False.not(), True);
        assert_eq!(Unknown.not(), Unknown);
        assert_eq!(True.not(), False);
    }
}

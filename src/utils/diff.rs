use std::fmt;

pub struct DiffStats {
    pub additions: usize,
    pub deletions: usize,
}

impl DiffStats {
    pub fn from_diff(diff: &str) -> Self {
        let mut additions = 0;
        let mut deletions = 0;

        for line in diff.lines() {
            if line.starts_with('+') && !line.starts_with("+++") {
                additions += 1;
            } else if line.starts_with('-') && !line.starts_with("---") {
                deletions += 1;
            }
        }

        Self {
            additions,
            deletions,
        }
    }
}

impl fmt::Display for DiffStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let total = self.additions + self.deletions;
        let filled = if total == 0 {
            0
        } else {
            (self.additions * 5) / total
        };

        let dots = "●".repeat(filled) + &"⊗".repeat(5 - filled);
        write!(f, "+{} -{} {dots}", self.additions, self.deletions)
    }
}

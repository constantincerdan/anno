pub mod pull_request;
pub mod repository;
pub mod workflows;

pub use pull_request::PullRequest;
pub use repository::Repository;

pub const IGNORED_REPO_PATHS: [&str; 9] = [
    ".github",
    "build",
    "Cargo.lock",
    "coverage",
    "dist",
    "target",
    "node_modules",
    "package-lock.json",
    "yarn.lock",
];

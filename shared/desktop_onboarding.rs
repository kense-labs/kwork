use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Step {
    #[default]
    Welcome,
    Credit,
    Purpose,
    Process,
    Done,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Purpose {
    Office,
    Development,
    Both,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Process {
    Compact,
    Standard,
    Detailed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Completion {
    Completed,
    Skipped,
    ApiKey,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Usage {
    #[default]
    Compact,
    Detailed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Progress {
    pub(crate) version: u8,
    pub(crate) step: Step,
    pub(crate) purpose: Option<Purpose>,
    pub(crate) process: Option<Process>,
    pub(crate) completion: Option<Completion>,
    pub(crate) usage: Usage,
    pub(crate) developer_tools: bool,
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            version: 1,
            step: Step::Welcome,
            purpose: None,
            process: None,
            completion: None,
            usage: Usage::Compact,
            developer_tools: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    pub(crate) revision: u64,
    pub(crate) progress: Progress,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Reply {
    pub(crate) snapshot: Snapshot,
    pub(crate) conflict: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub(crate) enum Action {
    Read,
    Next,
    Back,
    SelectPurpose { purpose: Option<Purpose> },
    SelectProcess { process: Process },
    Finish { completion: Completion },
}

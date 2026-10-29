//! Projects: a folder on disk the user works in, and the folders that group its threads.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
}

/// A folder inside a project. Its threads are those whose `folder_id` names it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: String,
    pub project_id: String,
    pub name: String,
}

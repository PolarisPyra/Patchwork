//! Linear: its personal API key, kept in the system's credential store, and the open issues
//! assigned to the key's owner, read from its GraphQL API.

// Rust guideline compliant 2026-10-09

use std::time::Duration;

use serde::Deserialize;

use super::{Issue, IssueSource};
use crate::provider::api::{delete_secret, read_secret, store_secret};
use crate::{Core, Error};

/// The credential store's name for the Linear API key.
const KEY_NAME: &str = "issues:linear";

/// Linear's GraphQL endpoint.
const ENDPOINT: &str = "https://api.linear.app/graphql";

/// How long a request to Linear may take.
const TIMEOUT: Duration = Duration::from_secs(20);

/// The open issues assigned to the key's owner, most recently updated first.
const QUERY: &str = r#"query {
  viewer {
    assignedIssues(
      first: 100
      orderBy: updatedAt
      filter: { state: { type: { nin: ["completed", "canceled"] } } }
    ) {
      nodes { identifier title url description state { name } }
    }
  }
}"#;

#[derive(Deserialize)]
struct Reply {
    data: Option<Data>,
    #[serde(default)]
    errors: Vec<GraphError>,
}

#[derive(Deserialize)]
struct GraphError {
    message: String,
}

#[derive(Deserialize)]
struct Data {
    viewer: Viewer,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Viewer {
    assigned_issues: Nodes,
}

#[derive(Deserialize)]
struct Nodes {
    nodes: Vec<LinearIssue>,
}

#[derive(Deserialize)]
struct LinearIssue {
    identifier: String,
    title: String,
    url: String,
    #[serde(default)]
    description: Option<String>,
    state: Option<State>,
}

#[derive(Deserialize)]
struct State {
    name: String,
}

impl Core {
    /// Whether a Linear API key is saved.
    #[must_use]
    pub fn has_linear_key(&self) -> bool {
        read_secret(KEY_NAME).is_some_and(|k| !k.trim().is_empty())
    }

    /// Saves `key` as the Linear API key, or forgets the saved one when `key` is None.
    ///
    /// # Errors
    ///
    /// Returns an error when the credential store refuses the key.
    pub fn set_linear_key(&self, key: Option<&str>) -> Result<(), Error> {
        if let Some(key) = key.map(str::trim).filter(|k| !k.is_empty()) {
            return store_secret(KEY_NAME, key).map(drop);
        }
        delete_secret(KEY_NAME);
        Ok(())
    }

    /// The open Linear issues assigned to the owner of the saved API key.
    ///
    /// This waits on the network, so call it off the UI thread.
    ///
    /// # Errors
    ///
    /// Returns an error when no key is saved, Linear cannot be reached, or it refuses the key.
    pub fn linear_issues(&self) -> Result<Vec<Issue>, Error> {
        let key = read_secret(KEY_NAME)
            .filter(|k| !k.trim().is_empty())
            .ok_or("Add a Linear API key to list its issues.")?;
        let http: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .http_status_as_error(false)
            .build()
            .into();
        let mut response = http
            .post(ENDPOINT)
            .header("Authorization", key.trim())
            .header("Content-Type", "application/json")
            .send_json(serde_json::json!({ "query": QUERY }))
            .map_err(|e| Error::Message(format!("Could not reach Linear: {e}")))?;
        let status = response.status();
        if status == 401 || status == 403 {
            return Err("Linear refused the API key: check it in Linear's settings.".into());
        }
        let reply: Reply = response
            .body_mut()
            .read_json()
            .map_err(|e| Error::Message(format!("Linear answered {status}: {e}")))?;
        if let Some(e) = reply.errors.first() {
            return Err(Error::Message(format!("Linear: {}", e.message)));
        }
        let data = reply.data.ok_or("Linear sent no issues.")?;
        Ok(data
            .viewer
            .assigned_issues
            .nodes
            .into_iter()
            .map(|i| Issue {
                source: IssueSource::Linear,
                key: i.identifier,
                title: i.title,
                url: i.url,
                body: i.description.unwrap_or_default(),
                labels: i.state.map(|s| s.name).into_iter().collect(),
            })
            .collect())
    }
}

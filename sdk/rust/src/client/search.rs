//! Search API operations.

use reqwest::Method;
use uuid::Uuid;

use crate::{ClientError, KivalClient, SearchParams, SearchResponse, Transport};

impl<S> KivalClient<S>
where
    S: Transport,
{
    /// Searches visible workspace content.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ApiKeyRequired`] if no API key is configured.
    pub async fn search_workspace(
        &self,
        workspace_id: Uuid,
        params: &SearchParams,
    ) -> Result<SearchResponse, ClientError> {
        self.require_api_key()?;

        let mut url = self.api_url(&format!("/workspaces/{workspace_id}/search"))?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("q", &params.q);
            if let Some(categories) = &params.categories {
                pairs.append_pair("categories", categories);
            }
            if let Some(status) = params.status {
                pairs.append_pair("status", status.as_str());
            }
            if let Some(limit) = params.limit {
                pairs.append_pair("limit", &limit.to_string());
            }
            if let Some(mode) = params.mode {
                pairs.append_pair("mode", mode.as_str());
            }
            if let Some(case_sensitive) = params.case_sensitive {
                pairs.append_pair("case_sensitive", &case_sensitive.to_string());
            }
            if let Some(context) = params.context {
                pairs.append_pair("context", &context.to_string());
            }
            if let Some(include_history) = params.include_history {
                pairs.append_pair("include_history", &include_history.to_string());
            }
        }

        let request = self.request_url(&Method::GET, url)?;
        self.send_json(request).await
    }
}

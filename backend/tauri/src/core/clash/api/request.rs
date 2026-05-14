use anyhow::{Context, Result};
use reqwest::{StatusCode, header::HeaderMap};
use serde::Serialize;
use url::Url;

/// The Request Parameters
pub(crate) struct PerformRequest<D = (), Q = ()> {
    pub(crate) method: reqwest::Method,
    pub(crate) path: String,
    pub(crate) query: Option<Q>,
    pub(crate) data: Option<D>,
}

/// A newtype wrapper for query parameters
pub(crate) struct Query<T>(pub(crate) T);
/// A newtype wrapper for request body
pub(crate) struct Data<T>(pub(crate) T);

impl From<(reqwest::Method, &str)> for PerformRequest<(), ()> {
    fn from((method, path): (reqwest::Method, &str)) -> Self {
        Self {
            method,
            path: path.to_string(),
            data: None,
            query: None,
        }
    }
}

impl<T> From<(reqwest::Method, &str, Data<T>)> for PerformRequest<T, ()>
where
    T: Serialize,
{
    fn from((method, path, Data(data)): (reqwest::Method, &str, Data<T>)) -> Self {
        Self {
            method,
            path: path.to_string(),
            data: Some(data),
            query: None,
        }
    }
}

impl<T> From<(reqwest::Method, &str, Query<T>)> for PerformRequest<(), T>
where
    T: Serialize,
{
    fn from((method, path, Query(query)): (reqwest::Method, &str, Query<T>)) -> Self {
        Self {
            method,
            path: path.to_string(),
            data: None,
            query: Some(query),
        }
    }
}

impl<D, Q> From<(reqwest::Method, &str, Query<Q>, Data<D>)> for PerformRequest<D, Q>
where
    D: Serialize,
    Q: Serialize,
{
    fn from(
        (method, path, Query(query), Data(data)): (reqwest::Method, &str, Query<Q>, Data<D>),
    ) -> Self {
        Self {
            method,
            path: path.to_string(),
            data: Some(data),
            query: Some(query),
        }
    }
}

/// Returns the controller base URL and auth headers from the global clash config.
fn clash_client_info() -> Result<(String, HeaderMap)> {
    let client = { crate::config::Config::clash().data().get_client_info() };

    let server = format!("http://{}", client.server);

    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse()?);

    if let Some(secret) = client.secret {
        let secret = format!("Bearer {secret}").parse()?;
        headers.insert("Authorization", secret);
    }

    Ok((server, headers))
}

#[tracing_attributes::instrument(skip_all, fields(
    method = tracing::field::Empty,
    url = tracing::field::Empty,
    query = tracing::field::Empty,
    data = tracing::field::Empty,
))]
pub(crate) async fn perform_request<D, Q>(
    param: impl Into<PerformRequest<D, Q>>,
) -> Result<reqwest::Response>
where
    Q: Serialize + core::fmt::Debug,
    D: Serialize + core::fmt::Debug,
{
    let PerformRequest {
        method,
        path,
        data,
        query,
    } = param.into();
    let (host, headers) = clash_client_info().context("failed to get clash client info")?;
    let base_url = Url::parse(&host).context("failed to parse host")?;
    let opts = url::Url::options().base_url(Some(&base_url));
    let url = opts.parse(&path).context("failed to parse path")?;

    let span = tracing::Span::current();
    span.record("method", tracing::field::display(&method));
    span.record("url", tracing::field::display(&url));
    span.record("query", tracing::field::debug(&query));
    span.record("data", tracing::field::debug(&data));

    async {
        let client = reqwest::ClientBuilder::new().no_proxy().build()?;
        let mut builder = client.request(method.clone(), url.clone()).headers(headers);

        if let Some(query) = &query {
            builder = builder.query(query);
        }
        if let Some(data) = &data {
            builder = builder.json(data);
        }

        let resp = builder.send().await?;

        if let Err(err) = resp.error_for_status_ref() {
            match err.status() {
                // Try To parse error message
                Some(StatusCode::BAD_REQUEST) => {
                    let Ok(bytes) = resp.bytes().await else {
                        return Err(err.into());
                    };

                    let message: serde_json::Value = match serde_json::from_slice(&bytes) {
                        Ok(v) => v,
                        Err(_) => {
                            let s = String::from_utf8_lossy(&bytes);
                            serde_json::Value::String(s.to_string())
                        }
                    };

                    return Err(err).context(format!("message: {message}"));
                }
                _ => return Err(err).context("clash api error"),
            }
        }
        Ok(resp)
    }
    .await
    .inspect_err(|e| tracing::error!(method = %method, url = %url, query = ?query, data = ?data, "failed to perform request: {:?}", e))
}

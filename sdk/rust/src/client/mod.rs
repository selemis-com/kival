//! Client for Kival.

use std::time::Duration;

use reqwest::Client;
use tower::{
    Layer,
    layer::util::{Identity, Stack},
};
use url::Url;

mod auth;
mod commentary;
mod error;
mod events;
mod graph;
mod groups;
mod objects;
mod search;
mod status;
mod transport;
mod users;
mod workspaces;

#[cfg(test)]
mod tests;

pub use error::{
    ApiError, ApiErrorKind, BaseUrlError, ClientError, TransportError, TransportErrorKind, UrlError,
};
pub use objects::ObjectVersionIdentifier;
pub use transport::{
    BoxTransport, BoxTransportFuture, HttpTransport, MapTransportError, ResponseTransport,
    Transport, append_event_params, append_list_params,
};

/// Default classified Reqwest transport used by [`KivalClient`].
pub type DefaultTransport = MapTransportError<ResponseTransport<HttpTransport>>;

/// Bearer API key retained by the client without exposing it through `Debug`.
#[derive(Clone)]
pub(crate) struct ApiKeyCredential(String);

impl std::fmt::Debug for ApiKeyCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ApiKeyCredential([REDACTED])")
    }
}

/// Builder for constructing a layered Kival client.
///
/// Layers follow [`tower::ServiceBuilder`] ordering: the first layer added receives the request
/// first and the response last.
#[derive(Debug, Clone)]
pub struct ClientBuilder<L = Identity> {
    /// Request timeout applied when the builder creates the Reqwest client.
    timeout: Duration,

    /// Optional caller-provided Reqwest client.
    http: Option<Client>,

    /// Optional bearer API key.
    api_key: Option<ApiKeyCredential>,

    /// Tower layer stack applied around classified Kival responses.
    layer: L,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self { timeout: Duration::from_secs(30), http: None, api_key: None, layer: Identity::new() }
    }
}

impl ClientBuilder<Identity> {
    /// Creates a new client builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl<L> ClientBuilder<L> {
    /// Sets the request timeout used when the builder creates its own Reqwest client.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Uses a caller-provided Reqwest client.
    ///
    /// When supplied, the client's own timeout and redirect configuration take precedence over
    /// [`Self::with_timeout`].
    #[must_use]
    pub fn with_http_client(mut self, http: Client) -> Self {
        self.http = Some(http);
        self
    }

    /// Attaches a bearer API key to the constructed client.
    #[must_use]
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(ApiKeyCredential(api_key.into()));
        self
    }

    /// Adds a Tower layer around the classified Kival transport.
    ///
    /// Layers are evaluated in the order they are added. For `.layer(A).layer(B)`, requests flow
    /// through `A -> B -> transport` and responses flow back through `B -> A`.
    #[must_use]
    pub fn layer<NewLayer>(self, layer: NewLayer) -> ClientBuilder<Stack<NewLayer, L>> {
        ClientBuilder {
            timeout: self.timeout,
            http: self.http,
            api_key: self.api_key,
            layer: Stack::new(layer, self.layer),
        }
    }

    /// Connects to a Kival server from a server-root URL string.
    ///
    /// # Errors
    ///
    /// Returns an error if the URL is invalid, the server-root contract is violated, or the
    /// Reqwest client cannot be constructed.
    pub fn connect(
        self,
        base_url: impl AsRef<str>,
    ) -> Result<KivalClient<MapTransportError<L::Service>>, ClientError>
    where
        L: Layer<ResponseTransport<HttpTransport>>,
        MapTransportError<L::Service>: Transport,
    {
        self.connect_http(base_url.as_ref().parse()?)
    }

    /// Builds a client for a parsed server-root URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the server-root contract is violated or the Reqwest client cannot be
    /// constructed.
    pub fn connect_http(
        self,
        base_url: Url,
    ) -> Result<KivalClient<MapTransportError<L::Service>>, ClientError>
    where
        L: Layer<ResponseTransport<HttpTransport>>,
        MapTransportError<L::Service>: Transport,
    {
        let http = self.request_client()?;
        let transport = HttpTransport::new(http.clone());
        self.finish(base_url, http, transport)
    }

    /// Builds a client with a caller-provided Reqwest client.
    ///
    /// # Errors
    ///
    /// Returns an error if the server-root contract is violated.
    pub fn connect_reqwest(
        self,
        http: Client,
        base_url: Url,
    ) -> Result<KivalClient<MapTransportError<L::Service>>, ClientError>
    where
        L: Layer<ResponseTransport<HttpTransport>>,
        MapTransportError<L::Service>: Transport,
    {
        self.with_http_client(http).connect_http(base_url)
    }

    /// Builds a client with a caller-provided raw request transport.
    ///
    /// The builder's Reqwest client is still used to construct `reqwest::Request` values. The
    /// supplied transport executes those requests, making this suitable for test transports,
    /// recording services, or alternate HTTP executors.
    ///
    /// # Errors
    ///
    /// Returns an error if the server-root contract is violated or the request-building Reqwest
    /// client cannot be constructed.
    pub fn connect_with_transport<T>(
        self,
        base_url: Url,
        transport: T,
    ) -> Result<KivalClient<MapTransportError<L::Service>>, ClientError>
    where
        L: Layer<ResponseTransport<T>>,
        MapTransportError<L::Service>: Transport,
    {
        let http = self.request_client()?;
        self.finish(base_url, http, transport)
    }

    /// Creates the Reqwest client used to construct requests.
    fn request_client(&self) -> Result<Client, ClientError> {
        match &self.http {
            Some(http) => Ok(http.clone()),
            None => Ok(Client::builder().timeout(self.timeout).build()?),
        }
    }

    /// Applies response classification and user middleware to a raw request transport.
    fn finish<T>(
        self,
        base_url: Url,
        http: Client,
        transport: T,
    ) -> Result<KivalClient<MapTransportError<L::Service>>, ClientError>
    where
        L: Layer<ResponseTransport<T>>,
        MapTransportError<L::Service>: Transport,
    {
        validate_base_url(&base_url)?;

        let transport = ResponseTransport::new(transport);
        let transport = self.layer.layer(transport);
        let transport = MapTransportError::new(transport);

        Ok(KivalClient { base_url, http, transport, api_key: self.api_key })
    }
}

/// Kival HTTP client for API-key-authenticated operations and public status endpoints.
#[derive(Debug, Clone)]
pub struct KivalClient<S = DefaultTransport> {
    /// Server root URL, for example `http://localhost:3000`.
    base_url: Url,

    /// Shared Reqwest client used to construct HTTP requests.
    http: Client,

    /// Tower service used to execute built HTTP requests.
    transport: S,

    /// Optional bearer API key.
    api_key: Option<ApiKeyCredential>,
}

impl KivalClient {
    /// Creates a new unauthenticated Kival client.
    ///
    /// `base_url` must be an HTTP or HTTPS origin root, for example `http://localhost:3000`.
    ///
    /// # Errors
    ///
    /// Returns an error if the server-root contract is violated or the Reqwest client cannot be
    /// constructed.
    pub fn new(base_url: Url) -> Result<Self, ClientError> {
        ClientBuilder::new().connect_http(base_url)
    }

    /// Creates a new unauthenticated Kival HTTP client.
    ///
    /// # Errors
    ///
    /// Returns an error if the server-root contract is violated or the Reqwest client cannot be
    /// constructed.
    pub fn new_http(base_url: Url) -> Result<Self, ClientError> {
        Self::new(base_url)
    }

    /// Connects to a Kival server from a server-root URL string.
    ///
    /// # Errors
    ///
    /// Returns an error if the URL is invalid, the server-root contract is violated, or the
    /// Reqwest client cannot be constructed.
    pub fn connect(base_url: impl AsRef<str>) -> Result<Self, ClientError> {
        ClientBuilder::new().connect(base_url)
    }
}

impl<S> KivalClient<S> {
    /// Returns this client authenticated with a bearer API key.
    #[must_use]
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(ApiKeyCredential(api_key.into()));
        self
    }

    /// Returns the configured server root URL.
    #[must_use]
    pub const fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// Returns the canonical browser URL for an object.
    ///
    /// The URL is derived from the server root configured for this client, so links point at the
    /// same Kival instance as API requests.
    #[must_use]
    pub fn object_url(&self, workspace_id: uuid::Uuid, object_id: uuid::Uuid) -> Url {
        let mut url = self.base_url.clone();
        url.set_path(&format!("/w/{workspace_id}/objects/{object_id}"));
        url
    }

    /// Returns the client's transport stack.
    #[must_use]
    pub const fn transport(&self) -> &S {
        &self.transport
    }

    /// Consumes the client and returns its transport stack.
    #[must_use]
    pub fn into_transport(self) -> S {
        self.transport
    }
}

impl<S> KivalClient<S>
where
    S: Transport,
{
    /// Type-erases the transport while retaining clone, send, and sync support.
    #[must_use]
    pub fn boxed(self) -> KivalClient<BoxTransport> {
        KivalClient {
            base_url: self.base_url,
            http: self.http,
            transport: BoxTransport::new(self.transport),
            api_key: self.api_key,
        }
    }
}

/// Validates the origin-root contract used by absolute Kival API paths.
fn validate_base_url(base_url: &Url) -> Result<(), ClientError> {
    match base_url.scheme() {
        "http" | "https" => {}
        scheme => return Err(ClientError::BaseUrl(BaseUrlError::UnsupportedScheme(scheme.into()))),
    }

    if !base_url.has_host() {
        return Err(ClientError::BaseUrl(BaseUrlError::MissingHost));
    }

    if !base_url.username().is_empty() || base_url.password().is_some() {
        return Err(ClientError::BaseUrl(BaseUrlError::Credentials));
    }

    if !matches!(base_url.path(), "" | "/") {
        return Err(ClientError::BaseUrl(BaseUrlError::PathPrefix));
    }

    if base_url.query().is_some() {
        return Err(ClientError::BaseUrl(BaseUrlError::Query));
    }

    if base_url.fragment().is_some() {
        return Err(ClientError::BaseUrl(BaseUrlError::Fragment));
    }

    Ok(())
}

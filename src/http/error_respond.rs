use crate::svc;
use super::box_body::{self, BoxBody};
use http::header::{HeaderValue, LOCATION};
use linkerd_app_core::{Error, Result, proxy::http::ClientHandle};
use linkerd_error_respond as respond;
use linkerd_stack::ExtractParam;
use std::{
    borrow::Cow,
};
use tracing::{debug, info_span};

pub fn layer<R, P: Clone, N>(
    params: P,
) -> impl svc::layer::Layer<N, Service = NewRespondService<R, P, N>> + Clone {
    respond::NewRespondService::layer(ExtractRespond(params))
}

pub type NewRespondService<R, P, N> =
    respond::NewRespondService<NewRespond<R>, ExtractRespond<P>, N>;

/// A strategy for responding to errors.
pub trait HttpRescue<E> {
    /// Attempts to synthesize a response from the given error.
    fn rescue(&self, error: E) -> Result<SyntheticHttpResponse, E>;
}

#[derive(Clone, Debug)]
pub struct SyntheticHttpResponse {
    // grpc_status: tonic::Code,
    http_status: http::StatusCode,
    close_connection: bool,
    message: Cow<'static, str>,
    location: Option<HeaderValue>,
}

#[derive(Copy, Clone, Debug)]
pub struct EmitHeaders(pub bool);

#[derive(Clone, Debug)]
pub struct ExtractRespond<P>(P);

#[derive(Copy, Clone, Debug)]
pub struct NewRespond<R> {
    rescue: R,
}

#[derive(Clone, Debug)]
pub struct Respond<R> {
    rescue: R,
    version: http::Version,
    is_grpc: bool,
    client: Option<ClientHandle>,
    accept: ContentType,
}

#[derive(Copy, Clone, Debug)]
enum ContentType {
    Html,
    Plaintext,
    Json,
    Grpc,
}

// const GRPC_STATUS: &str = "grpc-status";
// const GRPC_MESSAGE: &str = "grpc-message";

// === impl HttpRescue ===

impl<E, F> HttpRescue<E> for F
where
    F: Fn(E) -> Result<SyntheticHttpResponse, E>,
{
    fn rescue(&self, error: E) -> Result<SyntheticHttpResponse, E> {
        (self)(error)
    }
}

// === impl SyntheticHttpResponse ===

#[allow(dead_code)]
impl SyntheticHttpResponse {
    pub fn unexpected_error() -> Self {
        Self::internal_error("unexpected error")
    }

    pub fn internal_error(msg: impl Into<Cow<'static, str>>) -> Self {
        Self {
            close_connection: true,
            http_status: http::StatusCode::INTERNAL_SERVER_ERROR,
            // grpc_status: tonic::Code::Internal,
            message: msg.into(),
            location: None,
        }
    }

    pub fn bad_gateway(msg: impl ToString) -> Self {
        Self {
            close_connection: true,
            http_status: http::StatusCode::BAD_GATEWAY,
            // grpc_status: tonic::Code::Unavailable,
            message: Cow::Owned(msg.to_string()),
            location: None,
        }
    }

    pub fn gateway_timeout(msg: impl ToString) -> Self {
        Self {
            close_connection: true,
            http_status: http::StatusCode::GATEWAY_TIMEOUT,
            // grpc_status: tonic::Code::Unavailable,
            message: Cow::Owned(msg.to_string()),
            location: None,
        }
    }

    pub fn unavailable(msg: impl ToString) -> Self {
        Self {
            close_connection: true,
            http_status: http::StatusCode::SERVICE_UNAVAILABLE,
            // grpc_status: tonic::Code::Unavailable,
            message: Cow::Owned(msg.to_string()),
            location: None,
        }
    }

    pub fn unauthenticated(msg: impl ToString) -> Self {
        Self {
            http_status: http::StatusCode::FORBIDDEN,
            // grpc_status: tonic::Code::Unauthenticated,
            close_connection: false,
            message: Cow::Owned(msg.to_string()),
            location: None,
        }
    }

    pub fn permission_denied(msg: impl ToString) -> Self {
        Self {
            http_status: http::StatusCode::FORBIDDEN,
            // grpc_status: tonic::Code::PermissionDenied,
            close_connection: false,
            message: Cow::Owned(msg.to_string()),
            location: None,
        }
    }

    pub fn loop_detected(msg: impl ToString) -> Self {
        Self {
            http_status: http::StatusCode::LOOP_DETECTED,
            // grpc_status: tonic::Code::Aborted,
            close_connection: true,
            message: Cow::Owned(msg.to_string()),
            location: None,
        }
    }

    pub fn not_found(msg: impl ToString) -> Self {
        Self {
            http_status: http::StatusCode::NOT_FOUND,
            // grpc_status: tonic::Code::NotFound,
            close_connection: false,
            message: Cow::Owned(msg.to_string()),
            location: None,
        }
    }

    pub fn redirect(http_status: http::StatusCode, location: &http::Uri) -> Self {
        Self {
            http_status,
            // grpc_status: tonic::Code::NotFound,
            close_connection: false,
            message: Cow::Borrowed("redirected"),
            location: Some(
                HeaderValue::try_from(location.to_string())
                    .expect("location must be a valid header value"),
            ),
        }
    }

    pub fn response(http_status: http::StatusCode, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            http_status,
            location: None,
            // grpc_status: tonic::Code::FailedPrecondition,
            close_connection: false,
            message: message.into(),
        }
    }

    // pub fn grpc(grpc_status: tonic::Code, message: impl Into<Cow<'static, str>>) -> Self {
    //     Self {
    //         grpc_status,
    //         http_status: http::StatusCode::OK,
    //         location: None,
    //         close_connection: false,
    //         message: message.into(),
    //     }
    // }

    #[inline]
    fn http_response(
        &self,
        version: http::Version,
        content_type: ContentType,
    ) -> http::Response<super::BoxBody> {
        #![allow(clippy::declare_interior_mutable_const)]

        const VERSION: &str = concat!("multipass/", env!("CARGO_PKG_VERSION"));
        const SERVER_HEADER: http::HeaderValue = http::HeaderValue::from_static(VERSION);
        const CLOSE_HEADER: http::HeaderValue = http::HeaderValue::from_static("close");

        debug!(
            status = %self.http_status,
            ?version,
            close = %self.close_connection,
            ?content_type,
            "Handling error on HTTP connection"
        );
        let mut rsp = http::Response::builder()
            .status(self.http_status)
            .version(version)
            .header(http::header::SERVER, SERVER_HEADER);

        if self.close_connection && version == http::Version::HTTP_11 {
            // Notify the (proxy or non-proxy) client that the connection will be closed.
            rsp = rsp.header(http::header::CONNECTION, CLOSE_HEADER);
        }

        if let Some(loc) = &self.location {
            rsp = rsp.header(LOCATION, loc);
        }

        let message = match content_type {
            ContentType::Plaintext => {
                rsp = rsp.header(http::header::CONTENT_TYPE, ContentType::PLAINTEXT);
                match self.message {
                    Cow::Borrowed(msg) => bytes::Bytes::from_static(msg.as_bytes()),
                    Cow::Owned(ref msg) => bytes::Bytes::copy_from_slice(msg.as_bytes()),
                }
            },
            ContentType::Html => {
                rsp = rsp.header(http::header::CONTENT_TYPE, ContentType::HTML);
                bytes::Bytes::from(format!(
"<!DOCTYLE html>
<html>
    <head>
        <meta charset=\"utf-8\">
        <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">
        <title>{status}</title>
    </head>
    <body>
        <h1>{status}</h1>
        <p>{message}</p>
        <p><em>{VERSION}</em></p>
    </body>
</html>",
                    status = self.http_status, message = self.message,
                ))
            },
            ContentType::Json => {
                rsp = rsp.header(http::header::CONTENT_TYPE, ContentType::JSON);
                bytes::Bytes::from(format!(
                    "{{\"status\":{}, \"message\": \"{}\"}}",
                    self.http_status.as_u16(),
                    self.message,
                ))
            },
            _ => unimplemented!("handle gRPC responses"),
        };
        rsp.body(box_body::boxed(http_body_util::Full::new(message))).unwrap()
    }
}

// === impl ExtractRespond ===

impl<T, R, P> ExtractParam<NewRespond<R>, T> for ExtractRespond<P>
where
    P: ExtractParam<R, T>,
{
    #[inline]
    fn extract_param(&self, t: &T) -> NewRespond<R> {
        NewRespond {
            rescue: self.0.extract_param(t),
        }
    }
}

// === impl NewRespond ===

impl<B, R> respond::NewRespond<http::Request<B>> for NewRespond<R>
where
    R: Clone,
{
    type Respond = Respond<R>;

    fn new_respond(&self, req: &http::Request<B>) -> Self::Respond {
        let client = req.extensions().get::<ClientHandle>().cloned();
        // debug_assert!(client.is_some(), "Missing client handle");

        let rescue = self.rescue.clone();
        let accept = req.headers().get(http::header::ACCEPT).and_then(|accept| accept.to_str().map(Some).unwrap_or_else(|error| { tracing::warn!(%error, "accept header should be UTF-8"); None }));
        let accept = match accept {
            Some(s) if s.contains(ContentType::JSON) => ContentType::Json,
            Some(s) if s.contains(ContentType::GRPC) => ContentType::Grpc,
            Some(s) if s.contains(ContentType::HTML) => ContentType::Html,
            Some(s) if s.contains(ContentType::PLAINTEXT) => ContentType::Plaintext,
            _ => ContentType::Html,
        };
        match req.version() {
            http::Version::HTTP_2 => {
                let is_grpc = req
                    .headers()
                    .get(http::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok().map(|s| s.starts_with(ContentType::GRPC)))
                    .unwrap_or(false);
                Respond {
                    client,
                    rescue,
                    is_grpc,
                    version: http::Version::HTTP_2,
                    accept,
                }
            }
            version => {
                Respond {
                    client,
                    rescue,
                    version,
                    is_grpc: false,
                    accept,
                }
            }
        }
    }
}

// === impl Respond ===

impl<R> Respond<R> {
    fn client_addr(&self) -> std::net::SocketAddr {
        self.client
            .as_ref()
            .map(|ClientHandle { addr, .. }| *addr)
            .unwrap_or_else(|| {
                tracing::debug!("Missing client address");
                ([0, 0, 0, 0], 0).into()
            })
    }
}

impl<R> respond::Respond<http::Response<BoxBody>, Error> for Respond<R>
where
    R: HttpRescue<Error> + Clone,
{
    type Response = http::Response<BoxBody>;

    fn respond(&self, res: Result<http::Response<BoxBody>>) -> Result<Self::Response> {
        let error = match res {
            Ok(rsp) => return Ok(rsp),
            Err(error) => error,
        };

        let rsp = info_span!("rescue", client.addr = %self.client_addr()).in_scope(|| {
            if !self.is_grpc {
                let version = self.version;
                tracing::info!(error, "{version:?} request failed",);
            } else {
                tracing::info!(error, "gRPC request failed");
            };
            self.rescue.rescue(error)
        })?;

        if rsp.close_connection {
            if let Some(ClientHandle { close, .. }) = self.client.as_ref() {
                close.close();
            } else {
                tracing::debug!("Missing client handle");
            }
        }

        let rsp = rsp.http_response(self.version, self.accept);

        Ok(rsp)
    }
}

// === impl ContentType ===

impl ContentType {
    const HTML: &'static str = "text/html";
    const PLAINTEXT: &'static str = "text/plain";
    const JSON: &'static str = "application/json";
    const GRPC: &'static str = "application/grpc";
}
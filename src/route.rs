use crate::discover::Name;
use http::uri;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct RoutingTable {
    routes: Arc<[(Recognize, Name)]>,
}

#[serde_with::serde_as]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Recognize {
    #[serde_as(as = "Option<serde_with::DisplayFromStr>")]
    #[serde(default)]
    pub host: Option<uri::Authority>,

    #[serde_as(as = "Option<serde_with::DisplayFromStr>")]
    #[serde(default)]
    pub path_regex: Option<regex::Regex>,
}

// === impl RoutingTable ===

impl<B> linkerd_router::SelectRoute<http::Request<B>> for RoutingTable {
    type Key = Name;
    type Error = anyhow::Error;

    /// Given a a request, returns the key matching this request.
    ///
    /// If no route matches the request, this method returns an error.
    fn select(&self, req: &http::Request<B>) -> Result<Self::Key, Self::Error> {
        self.routes
            .iter()
            .find_map(|(recognize, name)| recognize.matches(req).then(|| name.clone()))
            .ok_or_else(|| anyhow::anyhow!("no route for request {}", req.uri()))
    }
}

impl FromIterator<(Recognize, Name)> for RoutingTable {
    fn from_iter<T: IntoIterator<Item = (Recognize, Name)>>(iter: T) -> Self {
        Self {
            routes: iter.into_iter().collect(),
        }
    }
}

// === impl Recognize ===

impl Recognize {
    pub fn matches<B>(&self, req: &http::Request<B>) -> bool {
        if let Some(ref authority) = self.host {
            let host = authority.host();
            if req.uri().authority().map(uri::Authority::host) == Some(host) {
                tracing::debug!(host, "request `:authority` matches");
                return true;
            } else {
                tracing::trace!(host, "request `:authority` does not match");
            }

            let host_header = req
                .headers()
                .get(http::header::HOST)
                .and_then(|val| val.to_str().ok());
            if host_header == Some(host) {
                tracing::debug!(host, "request `Host` header matches");
                return true;
            } else {
                tracing::trace!(host, ?host_header, "request `Host` header does not match");
            }
        }

        if let Some(ref path_regex) = self.path_regex {
            if let Some(path) = req
                .uri()
                .path_and_query()
                .map(http::uri::PathAndQuery::path)
            {
                if path_regex.is_match(path) {
                    tracing::debug!(%path_regex, path, "request path matches");
                    return true;
                } else {
                    tracing::trace!(%path_regex, path, "request path does not match");
                }
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority() {
        crate::test_util::trace_init();

        let recognize = Recognize {
            host: Some("test.example.com".parse().unwrap()),
            path_regex: None,
        };

        recognize.assert_match(
            &http::Request::builder()
                .uri("http://test.example.com/")
                .body(())
                .unwrap(),
        );

        recognize.assert_not_match(
            &http::Request::builder()
                .uri("http://foo.example.com/")
                .body(())
                .unwrap(),
        );

        recognize.assert_not_match(
            &http::Request::builder()
                .uri("http://example.com/")
                .body(())
                .unwrap(),
        );

        recognize.assert_match(
            &http::Request::builder()
                .uri("http://test.example.com:8080/")
                .body(())
                .unwrap(),
        );

        recognize.assert_match(
            &http::Request::builder()
                .header("host", "test.example.com")
                .uri("/")
                .body(())
                .unwrap(),
        );

        recognize.assert_match(
            &http::Request::builder()
                .header("host", "test.example.com")
                .uri("/foo/bar")
                .body(())
                .unwrap(),
        );

        recognize.assert_not_match(
            &http::Request::builder()
                .header("host", "example.com")
                .uri("/")
                .body(())
                .unwrap(),
        );

        recognize.assert_not_match(
            &http::Request::builder()
                .header("host", "www.example.com")
                .uri("/")
                .body(())
                .unwrap(),
        );
    }

    impl Recognize {
        #[track_caller]
        fn assert_match(&self, req: &http::Request<()>) {
            assert!(dbg!(self.matches(dbg!(req))), "{req:?} should match");
        }

        #[track_caller]
        fn assert_not_match(&self, req: &http::Request<()>) {
            assert!(dbg!(!self.matches(dbg!(req))), "{req:?} should not match");
        }
    }
}

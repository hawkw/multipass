use http::header::{HeaderName, HeaderValue};
use linkerd_stack::{layer, ExtractParam, NewService};
use std::{pin::Pin, future::Future, task::{Context, Poll}};
use futures::TryFuture;

/// Wraps an HTTP `Service` so that the Stack's `T -typed target` is cloned into
/// each request and/or response's headers.
#[derive(Clone, Debug)]
pub struct NewHeaderFromTarget<H, X, N> {
    inner: N,
    extract: X,
    which: Which,
    _marker: std::marker::PhantomData<fn() -> H>,
}

#[derive(Clone, Debug)]
pub struct HeaderFromTarget<S> {
    name: HeaderName,
    value: HeaderValue,
    which: Which,
    inner: S,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Which { pub request: bool, pub response: bool }

#[pin_project::pin_project]
pub struct ResponseFuture<F> {
    #[pin]
    future: F,
    header: Option<HeaderPair>,
}

type HeaderPair = (HeaderName, HeaderValue);

// === impl NewHeaderFromTarget ===

impl<H, X: Clone, N> NewHeaderFromTarget<H, X, N> {
    pub fn layer_via(which: Which, extract: X) -> impl layer::Layer<N, Service = Self> + Clone {
        layer::mk(move |inner| Self {
            inner,
            which,
            extract: extract.clone(),
            _marker: std::marker::PhantomData,
        })
    }
}

// impl<H, N> NewHeaderFromTarget<H, (), N> {
//     pub fn layer() -> impl layer::Layer<N, Service = Self> + Clone {
//         Self::layer_via(())
//     }
// }

impl<H, T, X, N> NewService<T> for NewHeaderFromTarget<H, X, N>
where
    H: Into<HeaderPair>,
    X: ExtractParam<H, T>,
    N: NewService<T>,
{
    type Service = HeaderFromTarget<N::Service>;

    fn new_service(&self, t: T) -> Self::Service {
        let (name, value) = self.extract.extract_param(&t).into();
        let inner = self.inner.new_service(t);
        HeaderFromTarget { which: self.which, name, value, inner }
    }
}

// === impl HeaderFromTarget ===

impl<S, ReqB, RspB> tower::Service<http::Request<ReqB>> for HeaderFromTarget<S>
where
    S: tower::Service<http::Request<ReqB>, Response = http::Response<RspB>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    #[inline]
    fn call(&mut self, mut req: http::Request<ReqB>) -> Self::Future {
        if self.which.request {
            req.headers_mut()
            .insert(self.name.clone(), self.value.clone());
        }
        let future = self.inner.call(req);
        ResponseFuture {
            future,
            header: self.which.response.then(|| (self.name.clone(), self.value.clone()))
        }
    }
}

// === impl ResponseFuture ===

impl<F, B> Future for ResponseFuture<F>
where F: TryFuture<Ok = http::Response<B>>,
{
    type Output = Result<F::Ok, F::Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let rsp = futures::ready!(this.future.try_poll(cx));
        Poll::Ready(rsp.map(|mut rsp| {
            if let Some((name, value)) = this.header.take() {
                rsp.headers_mut().insert(name, value);
            }
            rsp
        }))
    }
}
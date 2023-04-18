//! A middleware that boxes HTTP response bodies.
// use crate::EraseResponse;
use futures::{future, TryFutureExt};
use http::{HeaderMap, HeaderValue};
use hyper::body::{Body, Frame, SizeHint};
use linkerd_app_core::Error;
use linkerd_stack::{layer, Proxy, Service};
use pin_project::pin_project;
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct BoxBody {
    inner: Pin<Box<dyn Body<Data = Data, Error = Error> + Send + 'static>>,
}

#[pin_project]
pub struct Data {
    #[pin]
    inner: Box<dyn bytes::Buf + Send + 'static>,
}

#[derive(Clone, Debug)]
pub struct BoxResponse<S>(S);

#[pin_project]
struct Inner<B: Body>(#[pin] B);

struct NoBody;

impl Default for BoxBody {
    fn default() -> Self {
        Self {
            inner: Box::pin(NoBody),
        }
    }
}

impl BoxBody {
    pub fn new<B>(inner: B) -> Self
    where
        B: Body + Send + 'static,
        B::Data: Send + 'static,
        B::Error: Into<Error>,
    {
        Self {
            inner: Box::pin(Inner(inner)),
        }
    }
}

impl Body for BoxBody {
    type Data = Data;
    type Error = Error;

    #[inline]
    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    #[inline]
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        self.as_mut().inner.as_mut().poll_frame(cx)
    }

    #[inline]
    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

impl bytes::Buf for Data {
    fn remaining(&self) -> usize {
        self.inner.remaining()
    }

    fn chunk(&self) -> &[u8] {
        self.inner.chunk()
    }

    fn advance(&mut self, n: usize) {
        self.inner.advance(n)
    }

    fn chunks_vectored<'a>(&'a self, dst: &mut [std::io::IoSlice<'a>]) -> usize {
        self.inner.chunks_vectored(dst)
    }
}

impl<B> Body for Inner<B>
where
    B: Body,
    B::Data: Send + 'static,
    B::Error: Into<Error>,
{
    type Data = Data;
    type Error = Error;

    #[inline]
    fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let opt = futures::ready!(self.project().0.poll_frame(cx));
        Poll::Ready(opt.map(|res| {
            res.map_err(Into::into).map(|buf| Data {
                inner: Box::new(buf),
            })
        }))
    }

    #[inline]
    fn size_hint(&self) -> SizeHint {
        self.0.size_hint()
    }
}

impl std::fmt::Debug for BoxBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoxBody").finish()
    }
}

impl Body for NoBody {
    type Data = Data;
    type Error = Error;

    fn is_end_stream(&self) -> bool {
        true
    }

    fn poll_frame(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        Poll::Ready(Ok(None))
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(0)
    }
}

impl<S> BoxResponse<S> {
    pub fn layer() -> impl layer::Layer<S, Service = Self> + Clone + Copy {
        layer::mk(Self)
    }

    // /// Constructs a boxing layer that erases the inner response type with [`EraseResponse`].
    // pub fn erased() -> impl layer::Layer<S, Service = EraseResponse<S>> + Clone + Copy {
    //     EraseResponse::layer()
    // }
}

impl<S, Req, B> Service<Req> for BoxResponse<S>
where
    S: Service<Req, Response = http::Response<B>>,
    B: Body + Send + 'static,
    B::Data: Send + 'static,
    B::Error: Into<Error> + 'static,
{
    type Response = http::Response<BoxBody>;
    type Error = S::Error;
    type Future = future::MapOk<S::Future, fn(S::Response) -> Self::Response>;

    #[inline]
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }

    #[inline]
    fn call(&mut self, req: Req) -> Self::Future {
        self.0.call(req).map_ok(|rsp| rsp.map(BoxBody::new))
    }
}

impl<Req, B, S, P> Proxy<Req, S> for BoxResponse<P>
where
    B: Body + Send + 'static,
    B::Data: Send + 'static,
    B::Error: Into<Error>,
    S: Service<P::Request>,
    P: Proxy<Req, S, Response = http::Response<B>>,
{
    type Request = P::Request;
    type Response = http::Response<BoxBody>;
    type Error = P::Error;
    type Future = future::MapOk<P::Future, fn(P::Response) -> Self::Response>;

    #[inline]
    fn proxy(&self, inner: &mut S, req: Req) -> Self::Future {
        self.0.proxy(inner, req).map_ok(|rsp| rsp.map(BoxBody::new))
    }
}

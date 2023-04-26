//! A middleware that boxes HTTP response bodies.
// use crate::EraseResponse;
use crate::svc;
use futures::{future, TryFutureExt};
use http_body::{Body, Frame};
use http_body_util::{BodyExt, combinators};
use linkerd_app_core::Error;
use linkerd_stack::{layer, Proxy, Service};
use pin_project::pin_project;
use std::task::{Context, Poll};

pub type BoxBody = combinators::UnsyncBoxBody<Data, Error>;

#[pin_project]
pub struct Data {
    #[pin]
    inner: Box<dyn bytes::Buf + Send + 'static>,
}

#[derive(Clone, Debug)]
pub struct BoxResponse<S>(S);

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

// === impl BoxResponse ===

impl<S> BoxResponse<S> {
    pub fn layer() -> impl svc::Layer<S, Service = Self> + Copy {
        layer::mk(|inner| Self(inner))
    }
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
        self.0.call(req).map_ok(|rsp| rsp.map(boxed))
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
        self.0.proxy(inner, req).map_ok(|rsp| rsp.map(boxed))
    }
}

pub fn boxed(body: impl Body<Data = impl bytes::Buf + Send + 'static, Error = impl Into<Error> + 'static> + Send + 'static) -> BoxBody {
    body.map_frame(|frame| {
        match frame.into_data() {
            Ok(data) => Frame::data(Data { inner: Box::new(data) }),
            Err(frame) => match frame.into_trailers() {
                Ok(trailers) => Frame::trailers(trailers),
                Err(_) => unreachable!("frame said it was not data, so it must be trailers..."),
            }
        }
    }).map_err(Into::into).boxed_unsync()
}
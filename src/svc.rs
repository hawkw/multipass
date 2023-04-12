use layer::Layer;
pub use linkerd_stack::*;
use std::task::{Context, Poll};

#[derive(Clone, Debug)]
pub struct Stack<S>(S);

pub fn stack<S>(inner: S) -> Stack<S> {
    Stack(inner)
}

impl<S> Stack<S> {
    pub fn push<L: Layer<S>>(self, layer: L) -> Stack<L::Service> {
        Stack(layer.layer(self.0))
    }

    /// Wraps a `Service<T>` as a `Service<()>`.
    ///
    /// Each time the service is called, the `T`-typed request is cloned and
    /// issued into the inner service.
    pub fn push_new_thunk(self) -> Stack<NewThunk<S>> {
        self.push(layer::mk(NewThunk::new))
    }

    /// Wraps an inner `MakeService` to be a `NewService`.
    pub fn into_new_service(self) -> Stack<new_service::FromMakeService<S>> {
        self.push(new_service::FromMakeService::layer())
    }

    /// Assuming `S` implements `NewService` or `MakeService`, applies the given
    /// `L`-typed layer on each service produced by `S`.
    pub fn push_on_service<L: Clone>(self, layer: L) -> Stack<OnService<L, S>> {
        self.push(OnServiceLayer::new(layer))
    }

    pub fn into_inner(self) -> S {
        self.0
    }
}

impl<T, N> NewService<T> for Stack<N>
where
    N: NewService<T>,
{
    type Service = N::Service;

    fn new_service(&self, t: T) -> Self::Service {
        self.0.new_service(t)
    }
}

impl<T, S> Service<T> for Stack<S>
where
    S: Service<T>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }

    fn call(&mut self, t: T) -> Self::Future {
        self.0.call(t)
    }
}

//! Spawning tasks without a copy of tokio's task machinery per call site.
//!
//! `tokio::spawn` is generic over the future, so every call site compiles
//! its own task harness (poll, wake, cancel, drop, join): about a quarter
//! of arugulad's LLVM IR came from ~220 of them (2026-10-27). Boxing the
//! future first leaves one harness per output type. The box is one more
//! allocation per spawn, beside the one tokio makes for the task.

use std::future::Future;
use std::pin::Pin;

use tokio::runtime::Handle;
use tokio::task::JoinHandle;

type Boxed<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// `tokio::spawn`, boxed.
pub fn spawn<F>(f: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    tokio::spawn(Box::pin(f) as Boxed<F::Output>)
}

/// `Handle::spawn`, boxed.
pub fn spawn_on<F>(rt: &Handle, f: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    rt.spawn(Box::pin(f) as Boxed<F::Output>)
}

/// `tokio::task::spawn_blocking`, boxed.
pub fn spawn_blocking<F, T>(f: F) -> JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(Box::new(f) as Box<dyn FnOnce() -> T + Send>)
}

//! The async runtime. One per process: the desktop shell hands it to Tauri
//! (`tauri::async_runtime::set`), so Tauri commands and the core share the same threads.

use std::future::Future;
use std::sync::OnceLock;

use tokio::runtime::{Handle, Runtime};
use tokio::task::JoinHandle;

fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("undertone-rt")
            .build()
            .expect("cannot start the async runtime")
    })
}

pub fn handle() -> Handle {
    runtime().handle().clone()
}

pub fn spawn<F>(task: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    runtime().spawn(task)
}

pub fn spawn_blocking<F, R>(f: F) -> JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    runtime().spawn_blocking(f)
}

pub fn block_on<F: Future>(task: F) -> F::Output {
    runtime().block_on(task)
}

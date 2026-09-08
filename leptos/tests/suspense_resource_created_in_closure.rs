//! A closure inside `<Suspense>` that creates a `Resource` and reads it in the
//! same run can never settle: every re-run creates a fresh resource and a fresh
//! pending task. The framework can't wait such a closure out, but it must not
//! hang the response either (#4866). It gives up after a bounded number of
//! retries, logs a warning naming the pattern, and renders the closure's last
//! value.

#![cfg(feature = "ssr")]

use any_spawner::Executor;
use futures::StreamExt;
use leptos::prelude::*;
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

/// Mirrors `MAX_SETTLE_RETRIES` in `tachys::reactive_graph`.
const MAX_SETTLE_RETRIES: usize = 8;

fn app(runs: Arc<AtomicUsize>) -> impl IntoView {
    view! {
        <Suspense fallback=|| "loading">{move || {
            runs.fetch_add(1, Ordering::SeqCst);
            // anti-pattern: the resource is created by the render closure
            let res = Resource::new(
                || (),
                |_| async move {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    "never-settles".to_string()
                },
            );
            res.get()
        }}</Suspense>
    }
}

async fn assert_terminates(
    html: impl Future<Output = String>,
    runs: &Arc<AtomicUsize>,
) {
    let html = tokio::time::timeout(Duration::from_secs(5), html)
        .await
        .unwrap_or_else(|_| {
            panic!(
                "render hung; body ran {} times",
                runs.load(Ordering::SeqCst)
            )
        });
    // the closure's last value is rendered; it is `None` because the last
    // run created yet another resource that has not loaded
    assert!(
        !html.contains("never-settles"),
        "a resource created in the render closure can't have loaded by the \
         time its creating run is rendered; got: {html:?}"
    );
    // one dry walk, one first resolve run, then one run per retry
    assert_eq!(
        runs.load(Ordering::SeqCst),
        2 + MAX_SETTLE_RETRIES,
        "body runs should be bounded by the retry cap"
    );
}

#[tokio::test]
async fn out_of_order_gives_up_after_bounded_retries() {
    _ = Executor::init_tokio();
    let owner = Owner::new();
    owner.set();

    let runs = Arc::new(AtomicUsize::new(0));
    let html = app(Arc::clone(&runs))
        .to_html_stream_out_of_order()
        .collect::<String>();
    assert_terminates(html, &runs).await;
}

#[tokio::test]
async fn in_order_gives_up_after_bounded_retries() {
    _ = Executor::init_tokio();
    let owner = Owner::new();
    owner.set();

    let runs = Arc::new(AtomicUsize::new(0));
    let html = app(Arc::clone(&runs))
        .to_html_stream_in_order()
        .collect::<String>();
    assert_terminates(html, &runs).await;
}

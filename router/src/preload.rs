//! Speculative route prefetching.
//!
//! [`use_preloader`] returns a callback that warms the WebAssembly code
//! chunk(s) for whatever route a path matches — fetched, compiled and memoized
//! ahead of time — so a later navigation there renders without the lazy-load
//! round-trip.
//!
//! Unlike a navigation, prefetching runs **only** the matched route views' code
//! warm ([`ChooseView::preload_code`]); it does *not* construct route data, so a
//! destination's [`LazyRoute::data`](crate::LazyRoute::data) — and any server
//! functions it starts — is never invoked for a route the user has not visited.
//!
//! Requests are no-ops on the server and when called outside a [`Router`]. The
//! matching + warming happens inside [`Routes`]/[`FlatRoutes`], which own the
//! route definitions; [`use_preloader`] only enqueues a path, so it can be
//! called from anywhere in the tree (e.g. a sidebar that is a *sibling* of the
//! routed outlet, not a descendant).
//!
//! [`Router`]: crate::components::Router
//! [`Routes`]: crate::components::Routes
//! [`FlatRoutes`]: crate::components::FlatRoutes

#[cfg(not(feature = "ssr"))]
use crate::matching::{MatchInterface, MatchNestedRoutes, RouteDefs};
use reactive_graph::{
    owner::{provide_context, use_context},
    signal::ArcRwSignal,
    traits::Update,
};
#[cfg(not(feature = "ssr"))]
use std::{future::Future, pin::Pin};

/// A FIFO of paths awaiting a speculative code-warm.
///
/// Provided at the [`Router`](crate::components::Router) so it is reachable from
/// the whole app, and drained inside [`Routes`](crate::components::Routes) /
/// [`FlatRoutes`](crate::components::FlatRoutes), which hold the typed route
/// definitions. It carries only `String`s, so it is `Send + Sync` and can cross
/// a `provide_context` boundary the route definitions themselves cannot.
#[derive(Clone, Default)]
pub(crate) struct RoutePreloadQueue(ArcRwSignal<Vec<String>>);

impl RoutePreloadQueue {
    fn enqueue(&self, path: String) {
        self.0.update(|queue| queue.push(path));
    }

    /// Subscribe (so a later [`enqueue`](Self::enqueue) re-runs the draining
    /// effect) and take the pending paths *without* notifying — clearing
    /// untracked is what stops the effect from re-triggering itself into a loop.
    #[cfg(not(feature = "ssr"))]
    fn take(&self) -> Vec<String> {
        use reactive_graph::traits::{Track, UpdateUntracked};
        self.0.track();
        let mut taken = Vec::new();
        self.0
            .update_untracked(|queue| taken = std::mem::take(queue));
        taken
    }
}

/// Provide the prefetch request queue. Called once from
/// [`Router`](crate::components::Router).
pub(crate) fn provide_route_preload_queue() {
    provide_context(RoutePreloadQueue::default());
}

/// Returns a callback that speculatively warms the WebAssembly code for the
/// route a path matches, without navigating and without running the route's
/// data/server functions.
///
/// The returned closure enqueues the path; the actual match + warm runs inside
/// the [`Routes`](crate::components::Routes) /
/// [`FlatRoutes`](crate::components::FlatRoutes) that owns the route
/// definitions. Calling it is cheap, idempotent at the loader level (successful
/// loads are memoized), and a no-op on the server or outside a
/// [`Router`](crate::components::Router). It applies no rate limiting of its
/// own: warming the closure for a route fetches that route's whole chunk
/// closure, so callers should bound how many distinct routes they warm.
///
/// ```rust,ignore
/// let preload = use_preloader();
/// // e.g. when a nav link scrolls into view, or on pointerdown:
/// preload("/reports/quarterly");
/// ```
pub fn use_preloader() -> impl Fn(&str) + Clone {
    let queue = use_context::<RoutePreloadQueue>();
    move |path: &str| {
        if let Some(queue) = &queue {
            queue.enqueue(path.to_owned());
        }
    }
}

/// Wire up draining of the prefetch queue for a set of route definitions.
///
/// Called from [`Routes`](crate::components::Routes) /
/// [`FlatRoutes`](crate::components::FlatRoutes), where the typed `Defs` is in
/// scope. Client-only: prefetching is meaningless during SSR.
#[cfg(not(feature = "ssr"))]
pub(crate) fn setup_route_preloading<Defs>(routes: RouteDefs<Defs>)
where
    Defs: MatchNestedRoutes + Clone + Send + 'static,
{
    use any_spawner::Executor;
    use futures::future::join_all;
    use reactive_graph::effect::Effect;

    let Some(queue) = use_context::<RoutePreloadQueue>() else {
        return;
    };

    Effect::new(move |_| {
        for path in queue.take() {
            // The matcher works on the path only; drop any query/fragment.
            let path = match path.split_once(['?', '#']) {
                Some((path, _)) => path.to_owned(),
                None => path,
            };
            if let Some(matched) = routes.match_route(&path) {
                let mut warms = Vec::new();
                collect_preload_code(matched, &mut warms);
                Executor::spawn_local(async move {
                    join_all(warms).await;
                });
            }
        }
    });
}

/// Walk a matched route and its nested children, collecting a code-only warm
/// future for each level's view.
#[cfg(not(feature = "ssr"))]
fn collect_preload_code<Match>(
    matched: Match,
    out: &mut Vec<Pin<Box<dyn Future<Output = ()>>>>,
) where
    Match: MatchInterface,
{
    use crate::ChooseView;

    let (view, child) = matched.into_view_and_child();
    out.push(Box::pin(async move { view.preload_code().await }));
    if let Some(child) = child {
        collect_preload_code(child, out);
    }
}

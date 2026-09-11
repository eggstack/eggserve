//! Generic tunnel task ownership (Plan 199 Tracks D/F/G).
//!
//! Single helper for H1/H2 upgrade-backed tunnels: admission via the
//! server-wide `max_active_tunnels` budget, `OnUpgrade` awaiting (buffered
//! read-ahead preserved via Hyper's `Upgraded`), bounded duplex bridging to
//! an EggServe-owned [`TunnelIo`](crate::primitives::tunnel::TunnelIo), and
//! lifecycle-aware shutdown accounting. H3 uses the same admission/counter
//! vocabulary in `server::http3` with stream-local send/recv bridging.
//!
//! Ordinary denial never reaches here (service returns ordinary `Response`).
//! All payload bytes stay out of logs; only kinds/statuses are recorded.

use std::sync::Arc;

use crate::ops::{Event, EventKind, OpsContext, Severity};
use crate::primitives::request_lifecycle::RequestLifecycle;
use crate::primitives::tunnel::{TunnelAcceptance, TunnelIo, TunnelKind};

use super::activity::ConnectionActivity;

/// Admit one H1/H2 tunnel and spawn its tracked task.
///
/// - Tries the server-wide tunnel budget (`try_acquire_owned`, no queueing);
///   exhaustion returns `false` (caller renders deterministic 503, drops the
///   acceptance so the handler never runs and `OnUpgrade` fails safe).
/// - On admission, spawns via `activity.spawn_tunnel` (driver drains before
///   reporting completion, so H1 keeps the owning connection alive and no
///   detached task survives `wait()`).
/// - The task awaits `OnUpgrade`, bridges with bounded backpressure, runs the
///   downstream handler with a lifecycle clone, and releases the permit +
///   gauge exactly once.
///
/// Returns `true` when admitted (handshake should be sent), `false` on
/// exhaustion (caller must send 503 instead).
pub(crate) async fn admit_and_spawn_h1_h2(
    activity: &Arc<ConnectionActivity>,
    tunnel_semaphore: &Arc<tokio::sync::Semaphore>,
    ops: &OpsContext,
    conn_id: u64,
    lifecycle: RequestLifecycle,
    acceptance: TunnelAcceptance,
) -> bool {
    let permit = match tunnel_semaphore.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => {
            ops.counters()
                .tunnels_rejected
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            ops.emit(
                Event::new(
                    Severity::Warn,
                    EventKind::TunnelRejected,
                    "tunnel saturated: active tunnel limit",
                )
                .connection_id(conn_id),
            );
            return false;
        }
    };
    let kind = acceptance.kind;
    activity
        .spawn_tunnel(run_h1_h2_tunnel(
            permit,
            ops.clone(),
            conn_id,
            lifecycle,
            acceptance,
            kind,
        ))
        .await;
    true
}

async fn run_h1_h2_tunnel(
    _permit: tokio::sync::OwnedSemaphorePermit,
    ops: OpsContext,
    conn_id: u64,
    lifecycle: RequestLifecycle,
    acceptance: TunnelAcceptance,
    kind: TunnelKind,
) {
    let TunnelAcceptance {
        handler,
        upgrade,
        kind: _,
    } = acceptance;
    let upgrade = match upgrade {
        Some(u) => u,
        None => {
            // No transport upgrade (e.g. hand-constructed test capability):
            // fail safe without running the handler.
            ops.counters()
                .tunnel_upgrade_failures
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            ops.emit(
                Event::new(
                    Severity::Warn,
                    EventKind::TunnelUpgradeFailed,
                    "tunnel upgrade unavailable",
                )
                .connection_id(conn_id),
            );
            return;
        }
    };
    let upgraded = match upgrade.await {
        Ok(u) => u,
        Err(_) => {
            ops.counters()
                .tunnel_upgrade_failures
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            ops.emit(
                Event::new(
                    Severity::Debug,
                    EventKind::TunnelUpgradeFailed,
                    "tunnel upgrade failed",
                )
                .connection_id(conn_id),
            );
            return;
        }
    };
    ops.counters()
        .tunnels_accepted
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    ops.counters()
        .active_tunnels
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    ops.emit(
        Event::new(
            Severity::Info,
            EventKind::TunnelAccepted,
            format!("tunnel accepted: {kind}"),
        )
        .connection_id(conn_id),
    );
    let _active_guard = ActiveTunnelGuard { ops: ops.clone() };

    // Bounded duplex: one end for the downstream codec, one for the bridge.
    // H1 read-ahead bytes are already inside `Upgraded` (`read_buf`);
    // wrapping via `TokioIo` preserves them without loss.
    let (io_for_handler, io_for_bridge) = TunnelIo::pair();
    let handler_lifecycle = lifecycle.clone();
    let bridge_lifecycle = lifecycle.clone();
    let handler_join = tokio::spawn(async move {
        handler(io_for_handler, handler_lifecycle).await;
    });

    let mut transport = hyper_util::rt::TokioIo::new(upgraded);
    let mut bridge_end = io_for_bridge.into_duplex();
    let bridge_result = tokio::select! {
        result = tokio::io::copy_bidirectional(&mut bridge_end, &mut transport) => {
            Some(result)
        }
        _ = bridge_lifecycle.cancelled() => {
            None
        }
    };
    // Bridge ended (peer close / transport failure / lifecycle): ensure the
    // handler cannot linger without transport. Abort is safe: the handler
    // owns only TunnelIo + lifecycle, no raw transport.
    handler_join.abort();
    let _ = handler_join.await;
    if let Some(Err(_)) = bridge_result {
        // Transport copy failure is already terminal; no payload logged.
    }
    ops.emit(
        Event::new(
            Severity::Debug,
            EventKind::TunnelClosed,
            format!("tunnel closed: {kind}"),
        )
        .connection_id(conn_id),
    );
    // `_permit` + `_active_guard` release exactly once on drop.
}

struct ActiveTunnelGuard {
    ops: OpsContext,
}

impl Drop for ActiveTunnelGuard {
    fn drop(&mut self) {
        self.ops
            .counters()
            .active_tunnels
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

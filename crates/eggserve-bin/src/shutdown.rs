use tokio::sync::broadcast;

/// Resolve when a termination signal arrives (SIGINT/SIGTERM/SIGHUP on Unix,
/// Ctrl+C elsewhere). All handled signals trigger the same graceful shutdown
/// path; SIGHUP is treated as a graceful stop rather than keeping its default
/// immediate-terminate action, matching daemon-management expectations.
pub async fn shutdown_signal(tx: broadcast::Sender<()>) {
    let ctrl_c = tokio::signal::ctrl_c();

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(_) => {
                let _ = tx.send(());
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    #[cfg(unix)]
    let hangup = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(_) => {
                let _ = tx.send(());
            }
        }
    };

    #[cfg(not(unix))]
    let hangup = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
        _ = hangup => {},
    }

    let _ = tx.send(());
}

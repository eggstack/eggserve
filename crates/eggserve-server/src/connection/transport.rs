//! Transport progress instrumentation.
//!
//! `ProgressIo` records forward socket-write progress (and inbound activity)
//! for the write-no-progress timeout. Transparent to Hyper's HTTP/1 framing;
//! TCP, TLS, and caller-owned transports all flow through this point before
//! `TokioIo`.

use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::activity::ConnectionActivity;

/// Transport wrapper at the Plan 163 transport boundary that records
/// forward socket-write progress (and inbound activity) for the
/// write-no-progress timeout. Transparent to Hyper's HTTP/1 framing; works
/// for TCP, TLS, and caller-owned transports because all of them flow
/// through this point before [`TokioIo`].
pub(crate) struct ProgressIo<I> {
    inner: I,
    activity: Arc<ConnectionActivity>,
}

impl<I> ProgressIo<I> {
    pub(crate) fn new(inner: I, activity: Arc<ConnectionActivity>) -> Self {
        Self { inner, activity }
    }
}

impl<I: AsyncRead + Unpin> AsyncRead for ProgressIo<I> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let this = self.get_mut();
        match std::pin::Pin::new(&mut this.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                this.activity
                    .record_read(buf.filled().len().saturating_sub(before));
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}

impl<I: AsyncWrite + Unpin> AsyncWrite for ProgressIo<I> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        let this = self.get_mut();
        match std::pin::Pin::new(&mut this.inner).poll_write(cx, buf) {
            Poll::Ready(Ok(n)) => {
                this.activity.record_write(n);
                Poll::Ready(Ok(n))
            }
            other => other,
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        let this = self.get_mut();
        std::pin::Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        let this = self.get_mut();
        std::pin::Pin::new(&mut this.inner).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> Poll<Result<usize, std::io::Error>> {
        let this = self.get_mut();
        match std::pin::Pin::new(&mut this.inner).poll_write_vectored(cx, bufs) {
            Poll::Ready(Ok(n)) => {
                this.activity.record_write(n);
                Poll::Ready(Ok(n))
            }
            other => other,
        }
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

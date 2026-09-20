# Plan 232 raw capture notes

The full native and TLS harness outputs were captured during the same session
as the compact `results.json` summary. The source-tree SHA before the
correction commit was `c4719a4cb5ddfac42a3c18775f8e2c6b5f4b2cea`; the
correction itself was uncommitted during measurement and is identified by the
explicit implementation description in `results.json`.

The 64 KiB native run used a temporary default-only source override and was
restored to 128 KiB before the final build. The range probe used raw H1
requests with `Range: bytes=...` and checked status `206`, content length,
and every response byte at concurrency 1/16/64 for 64 KiB and 512 KiB
ranges. All range trials were exact.

The TLS capture used the Plan 170 harness with three warm-up-excluded trials,
established keep-alive static 1 KiB and 1 MiB cases at concurrency 1/16/64,
and separate 48-connection handshake churn. The HWM RSS field in the
keep-alive sequence is cumulative across that process's cases; it is retained
as observed resource evidence, not interpreted as a per-case allocation.


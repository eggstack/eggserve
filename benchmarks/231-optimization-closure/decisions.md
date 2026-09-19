# Plan 231 decisions

| Plan | Decision | Evidence and guardrail |
|------|----------|-----------------------|
| 228 | Keep | Direct H1 producer-poll progress was unused by the timeout driver; actual socket writes through `ProgressIo` remain authoritative. The connection-level `Arc<PipelineState<S>>` keeps the public `Service` contract unchanged. Direct H1 parity and lifecycle tests pass; compatibility H2 tracking remains intact. |
| 229 | Keep | `BytesMut` + `read_buf` removes zero-fill work without unsafe code or pooling. The 128 KiB default was selected by the Plan 227 body/frame matrix and remains bounded by `max_file_streams * stream_chunk_size`. Static conditional/range lookup and lowercase MIME lookup avoid the measured temporary work. Static wire and resource suites pass. |
| 230 | Keep | Runtime conversion no longer creates an intermediate origin `Date` when contextual finalization owns the policy; standalone conversion keeps its documented Date behavior. Header validation uses bounded first-value state, and lifecycle events use lazy sink capability checks. Privacy, observability, and direct/compatibility tests pass. |

No optimization was promoted based on a single timing number. No sendfile,
mmap cache, io_uring, custom allocator, buffer pool, or public Service API
change was introduced. H2/H3 protocol-specific producer semantics remain
unchanged.

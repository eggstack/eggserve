# Plan 194 — HTTP/3 Response-Producer Timeout and Promotion-Trace Correction

## Status

**CLOSED — implemented 2026-09-10; H3 remains experimental.**

Outcome: absolute producer no-progress deadline (`timeout_at`, empty chunks
excluded) with stream-scoped reset and `WriteStallTimeout` observability,
five Track G regression tests (H3 suite 9 → 14), Plan 193 preflight-blocked
trace correction, Plan 192 corrective note, and synchronized timeout docs.
No promotion granted; Plans 192/193 blockers stand. See
`release/plan-194-http3-producer-timeout-correction.md`. Handoff to Plan 195
for corrective qualification.

Baseline when written: `main` at `368fc2aad973bd4eecbc1d041ea3778630165f1c` (`Close Plan 193 HTTP/3 promotion attempt as experimental`). Implemented on top of `8128fe7 Add Plan 195 H3 response timeout qualification` (planning handoff). Re-read current source, plan status, release records, and documentation before implementation in case `main` has moved.

Prerequisites:

- Plans 183–190 remain implemented/closed at their recorded support tiers;
- Plan 191 remains a closed H2 promotion attempt with H2 still experimental;
- Plan 192 remains a closed H3 dependency-readiness gate with `BLOCKED` outcome;
- Plan 193 remains non-promoting: HTTP/3 is still experimental and no later plan has superseded its evidence boundary.

This plan corrects two narrow issues found by the post-Plan-193 review:

1. the H3 `ResponseBody::Stream` adapter currently applies `response_write_timeout` to H3 response-head/data/finish sends, but **not** to the wait for the next application-produced stream item (`response_stream.next().await`), so a producer that stops yielding after response commitment can remain parked without the claimed per-response no-progress bound; and
2. Plan 193's historical status/closure wording says the promotion attempt was “executed” even though Plan 192 had closed `BLOCKED` and Plan 193's own prerequisite explicitly said not to execute the promotion plan in that state. The repository correctly retained H3 as experimental, but the historical trace should say that Plan 193 was **preflight-blocked / closed without entering promotion qualification**, while preserving the useful evidence inventory and fail-closed gate probes it recorded.

This plan does **not** attempt HTTP/2 or HTTP/3 support-tier promotion, does not reopen the Plan 192 upstream blockers, does not add another timeout setting, and does not broaden the protocol or product surface.

## Purpose

Restore exact agreement among runtime behavior, timeout documentation, and plan history with the smallest maintainable change.

The desired end state is:

- an H3 streaming response cannot pin a request task indefinitely merely because its application producer stops yielding body items after response commitment;
- `response_write_timeout` remains a no-progress budget, not a total response-duration budget;
- slow-but-progressing streams remain valid;
- empty chunks cannot be used to perpetually refresh the no-progress budget;
- the existing H3 send/flow-control timeout remains stream-scoped;
- producer timeout uses the existing post-commit response-failure path and does not attempt a second HTTP response;
- siblings remain usable after one producer stalls;
- no new public configuration type or dependency is introduced;
- H1/H2 behavior is unchanged;
- Plan 193 and all live references distinguish “promotion preflight/evidence inventory performed” from “supported-tier promotion qualification executed”;
- the useful Plan 193 evidence record is retained rather than deleted or rewritten as though it never happened;
- HTTP/3 remains experimental after this correction.

## Current defect: H3 producer wait is outside the timeout

At the baseline, `send_canonical_response()` in `crates/eggserve-core/src/server/http3.rs` does all of the following correctly:

- wraps `stream.send_response(...)` in `response_write_timeout`;
- splits buffered/file output through bounded `send_bytes(...)` calls;
- wraps each H3 `send_data(...)` call in `response_write_timeout`;
- wraps `stream.finish()` in `response_write_timeout`;
- validates the declared length of a canonical `ResponseBody::Stream`;
- routes post-commit failure through `send_response_or_cancel()`, which cancels the request lifecycle and resets the send direction rather than generating a second response.

However, the stream loop is currently equivalent to:

```rust
while let Some(chunk) = response_stream.next().await {
    let chunk = chunk.map_err(...)?;
    if !chunk.is_empty() {
        send_bytes(stream, chunk, config).await?;
    }
}
```

The `next().await` itself has no deadline. Therefore:

- a producer that returns the `Response` promptly and then never yields a body item bypasses `handler_timeout`;
- no H3 `send_data` future exists yet, so `response_write_timeout` on the wire-send path cannot fire;
- `connection_total_timeout` intentionally does not apply to H3 after the Plan 192 lifetime decision;
- QUIC `max_idle_timeout` is transport-idle, not an application-response producer guarantee, and sibling/connection traffic may keep the connection active;
- server shutdown eventually aborts the task, but shutdown is not a normal runtime no-progress bound.

The Plan 192 readiness record therefore overstates the baseline when it says a non-yielding `ResponseStream` is bounded by `response_write_timeout`.

## Correct semantic contract

Reuse the existing `RuntimeConfig::response_write_timeout`; do **not** add `response_producer_timeout` or another public knob.

For H3 `ResponseBody::Stream`, define `response_write_timeout` as a per-response **body no-progress** budget spanning both application production and H3 send progress:

1. response HEADERS still have their existing one-operation send timeout;
2. after response HEADERS succeed and the runtime begins consuming a canonical body stream, arm a producer no-progress deadline at `now + response_write_timeout`;
3. waiting for the next meaningful producer result must not exceed that deadline;
4. an empty successful body chunk is **not meaningful forward progress** and must not reset the deadline;
5. a non-empty produced body chunk moves processing to the existing `send_bytes()` path, whose H3 `send_data` operations remain independently bounded by `response_write_timeout` under QUIC flow control;
6. after the non-empty chunk has been sent successfully, re-arm the producer deadline for the next body item;
7. clean EOS is completion, subject to the existing known-length equality check;
8. producer error remains immediate failure;
9. producer timeout after response commitment is a post-commit stream failure: do not attempt another status response; use the existing request-lifecycle cancellation + H3 send reset path;
10. sibling streams and the H3 connection remain usable unless the underlying H3/QUIC stack reports a connection-wide failure.

This gives the runtime a truthful no-progress guarantee without conflating application production with QUIC wire credit.

### Why the deadline should not reset on empty chunks

An application stream can legally yield an empty `Bytes`. Treating every `Poll::Ready(Some(Ok(Bytes::new())))` as progress would permit an empty-chunk loop to keep the timeout alive indefinitely while delivering no response bytes. The H3 adapter should therefore preserve one producer deadline across empty chunks.

Do not add arbitrary “maximum empty chunks” policy unless profiling/tests demonstrate a separate CPU-spin problem. The timeout/no-progress rule is sufficient for the correctness contract. If a producer emits an unbounded number of immediately-ready empty chunks without yielding to Tokio, a time-based future cannot preempt a continuously-ready loop; in that case add a small cooperative-yield mechanism only if a targeted regression demonstrates the need. Do not preemptively redesign `ResponseStream`.

## Track A — Implement the H3 producer no-progress deadline

Primary file:

- `crates/eggserve-core/src/server/http3.rs`

Expected implementation shape:

- keep the existing response-head timeout;
- in `ResponseBody::Stream`, create an absolute `tokio::time::Instant` deadline for producer progress;
- use `tokio::time::timeout_at(deadline, response_stream.next())` or an equivalent monotonic-deadline mechanism;
- do not create a fresh relative timeout before every empty chunk;
- on non-empty chunk production, send through the existing bounded `send_bytes()` path;
- after successful non-empty send, set the next producer deadline to `Instant::now() + config.response_write_timeout`;
- on EOS, preserve the current declared-length validation;
- on producer error, preserve the existing failure behavior;
- on producer timeout, return an internal error from `send_canonical_response()` that is handled by `send_response_or_cancel()` as a post-commit failure.

Do not expose transport-specific timeout types through the public canonical response API.

### Error string / internal representation

The baseline uses an internal `Result<(), String>` for H3 response sending. Do not introduce a new public error hierarchy solely for this correction. A narrow internal message such as `response producer timeout` is sufficient if the message does not leak to the client and observability remains generic/sanitized.

If the implementation naturally benefits from a tiny private enum differentiating producer timeout, send timeout, producer error, and length mismatch for counters/tests, that is acceptable only if it remains private and reduces string matching. Do not expand the public `ServiceError` or `ServerError` surface for a post-commit transport failure.

## Track B — Observability parity

Audit how H1/H2 response-write no-progress timeouts update `OpsContext` events/counters and how H3 send failures currently report cancellation.

The correction must make producer timeout observable enough to distinguish it from a normal response completion without creating a new schema unless necessary.

Preferred behavior:

- reuse `EventKind::WriteStallTimeout` / the existing write-stall counter for the no-progress timeout if doing so matches the established semantic definition;
- retain `ResponseStreamProducerError` for a producer that explicitly returns `Err`, not for a timeout;
- retain lifecycle cancellation as `TransportFailure` after response commitment unless a more precise existing cancellation reason already owns response no-progress;
- never log response bytes, raw application data, QUIC secrets, or dependency internals.

If H3 currently does not increment the write-stall counter for ordinary `send_data` timeout, either make producer/send timeout observability consistent in this narrow pass or document the intentional difference. Avoid adding an H3-only public metric for the same semantic timeout.

## Track C — Preserve transport and sibling scope

A producer no-progress timeout is a response-stream failure, not proof the H3 connection is unusable.

Acceptance behavior:

- terminate/reset only the affected H3 send direction through the existing H3 error path;
- cancel only that request's `RequestLifecycle` for the ordinary producer-timeout case;
- do not call connection-wide `requests_registry.cancel_all()` merely because one response producer stalled;
- keep sibling requests able to complete;
- do not alter the Plan 192 decision that actual H3 connection errors cancel all live lifecycle observers;
- do not change H1/H2 connection-level fallback behavior.

## Track D — Known-length and empty-stream semantics

Add/retain exact behavior for:

- known-length stream that yields exactly the declared bytes then EOS → succeeds;
- known-length stream that yields fewer bytes and then EOS → existing length-mismatch failure;
- known-length stream that yields more bytes → existing length-mismatch/failure behavior must remain bounded;
- unknown-length stream that yields data then EOS → succeeds;
- zero-length/empty stream that immediately returns EOS → succeeds;
- stream that yields one or more empty chunks then real bytes within the original no-progress deadline → succeeds and only meaningful data resets the subsequent deadline;
- stream that yields empty chunks and then parks beyond the original deadline → times out;
- stream that parks before its first item → times out after response commitment;
- stream that yields real data periodically at intervals shorter than the timeout → may continue for longer than one timeout interval overall.

Do not reinterpret `response_write_timeout` as a total streaming duration.

## Track E — Plan 193 historical trace correction

The implementation review found a bookkeeping inconsistency, not a support-tier correctness failure.

Plan 192's status says:

> Plan 193 must not execute until a later narrow readiness update resolves the blockers.

Plan 193's historical prerequisite says the same thing, but its current status says:

> promotion attempt executed 2026-09-10

The release record itself admits the prerequisite was unmet and that the pass only froze/rechecked the candidate, probed evidence availability, and retained experimental status.

Correct the history without erasing evidence:

### E1. Plan 193 status

Update the leading status/outcome in `plans/193-http3-supported-tier-promotion-qualification.md` to language equivalent to:

- **CLOSED — promotion preflight blocked 2026-09-10; supported-tier qualification was not entered; H3 remains experimental.**

Preserve the original plan below as historical content. Do not rewrite its original prerequisite or acceptance criteria.

### E2. Plan 193 release record

Update `release/plan-193-http3-supported-tier-qualification.md` so the top-level scope/decision accurately states:

- Plan 192 was `BLOCKED`;
- therefore the mandatory promotion qualification phase was not eligible to run;
- the work performed was a preflight/evidence inventory and fail-closed gate probe against the frozen candidate;
- no runtime source change or support promotion occurred;
- all captured tool/environment/blocker evidence remains useful for a future scoped promotion plan.

Do not delete test counts, environment details, blocker inventory, or gate results. Reframe only claims that say promotion qualification itself was executed.

### E3. Live references

Search all live repository documents for statements equivalent to “Plan 193 executed the promotion attempt.” Update them to the corrected historical wording. Likely locations include:

- `README.md`;
- `plans/ROADMAP.md`;
- `architecture/http3.md`;
- `docs/release-contract.md`;
- `docs/api-stability.md`;
- `.opencode/skills/eggserve-dev/SKILL.md`;
- `AGENTS.md`;
- any capability/timeout/dependency docs touched by Plans 192–193.

Use one concise canonical description to avoid drift: Plan 193's **preflight was blocked by Plan 192; evidence availability was inventoried; H3 remained experimental; a new scoped plan is required after readiness blockers resolve.**

Do not falsify immutable Git commit messages. Historical commit subjects can continue to say “Close Plan 193 ...”; only tracked documentation/plan content is corrected.

## Track F — Timeout documentation correction

Synchronize at least:

- `docs/timeout-reference.md`;
- `architecture/http3.md`;
- README/runtime documentation if it makes an H3 response no-progress claim;
- Plan 192 readiness record, with an explicit corrective note rather than silently editing its historical conclusion;
- Plan 193 record if it repeats the old producer-timeout statement.

The corrected H3 timeout wording should distinguish:

- response-head send timeout;
- application `ResponseStream` producer no-progress timeout;
- QUIC/H3 DATA send timeout under flow control;
- finish timeout;
- QUIC idle timeout;
- absence of `connection_total_timeout` on H3.

Recommended semantic wording:

> For H3 streaming responses, `response_write_timeout` is a per-response no-progress budget across application body production and each bounded H3 send operation. Meaningful produced body data followed by successful send re-arms the producer budget; empty chunks do not. QUIC connection lifetime remains governed by `Http3Config::max_idle_timeout` plus per-operation deadlines rather than `connection_total_timeout`.

Do not claim H3 wire bytes are observed directly beyond what Quinn/h3's awaited send operations guarantee.

## Track G — Regression tests

Primary test file:

- `crates/eggserve-core/tests/http3_runtime.rs`

Add focused deterministic tests using the existing in-process H3 harness. Avoid introducing a new external H3 client just for this correction.

Mandatory tests:

1. **producer never yields**
   - service returns response-start promptly with `ResponseBody::Stream`;
   - producer remains pending before first chunk;
   - configured short `response_write_timeout` expires;
   - the response/request task terminates and does not remain live indefinitely;
   - affected stream is reset/terminated post-commit rather than receiving a second response.

2. **producer stalls after progress**
   - producer yields one real chunk;
   - client receives that chunk;
   - producer then parks;
   - timeout is measured from the last meaningful progress/successful send, not from response creation;
   - stream terminates after the no-progress budget.

3. **slow but progressing producer**
   - producer yields multiple real chunks with intervals below the timeout;
   - total stream duration exceeds one timeout interval;
   - response completes successfully;
   - demonstrates no-progress rather than total-duration semantics.

4. **empty chunks do not refresh**
   - producer emits empty chunks and then parks;
   - timeout remains tied to the last meaningful progress point;
   - empty chunks cannot keep the response alive indefinitely.

5. **sibling isolation**
   - one H3 stream uses a stalled producer;
   - a sibling request completes while the first is stalled and remains usable after the stalled stream is terminated;
   - no connection-wide lifecycle cancellation occurs solely because of the producer timeout.

6. **resource cleanup**
   - request/service/file/connection counters or relevant observable task state return to the expected baseline after the timed-out stream is gone;
   - no semaphore permit is leaked.

Also keep existing H3 response-length, peer-close, shutdown, body-policy, and lifecycle tests green.

### Test timing discipline

Prefer deterministic synchronization channels/notifies over broad wall-clock sleeps. A short actual timeout is acceptable where Tokio time must drive the path, but tests should use a generous ratio between intended progress interval and timeout to avoid CI flakes. Use Tokio paused-time support only if already enabled/compatible; do not add a heavyweight test dependency for this correction.

## Track H — H1/H2 non-regression

Because `response_write_timeout` is shared configuration, explicitly prove the H3 change did not reinterpret H1/H2:

- no change to `ProgressIo` H1 socket-write semantics;
- no change to H2 producer/poll-progress accounting or its conservative connection-shutdown fallback;
- no new shared timeout field;
- no change to the Python compatibility facade;
- no new default feature/dependency.

Do not attempt to harmonize all three implementations beyond the documented protocol-specific observability limits in this plan.

## Track I — Support-tier and upstream blocker preservation

After Plan 194:

- H1 remains supported/default;
- H2 remains experimental under Plan 191's unresolved browser/platform/trailer evidence;
- H3 remains experimental;
- `hyperium/h3#338` remains a promotion blocker until a maintained released fix or later explicit disposition;
- the Plan 192 `#262` residual receive-termination paths remain promotion blockers unless independently fixed by another authorized plan;
- Plan 194 does not claim `READY FOR H3 PROMOTION`.

If implementation naturally fixes one of the residual `#262` paths while adding producer timeout, record it, but do not expand scope to chase all upstream blockers unless necessary for correctness of this change.

## Track J — Verification matrix

At minimum run:

```text
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-python-release-metadata.py
cargo fmt --all -- --check
cargo +1.88 check --workspace --all-targets --features http3,tls
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http2,tls
cargo clippy -p eggserve-core --features http3,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http3,tls
cargo clippy -p eggserve-bin --features http3,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http3,tls
cargo audit
cargo deny check
bash scripts/qualify-http3.sh
```

The ordinary H3 qualification script may still report external promotion evidence as unavailable/SKIP. That is expected and does not fail Plan 194; this is a deterministic corrective implementation pass, not support promotion.

## Expected files

Likely source/test changes:

- `crates/eggserve-core/src/server/http3.rs`;
- `crates/eggserve-core/tests/http3_runtime.rs`.

Likely historical/documentation changes:

- `plans/193-http3-supported-tier-promotion-qualification.md`;
- `release/plan-193-http3-supported-tier-qualification.md`;
- `release/plan-192-http3-dependency-readiness.md` (corrective note only if it contains the overstated producer bound);
- `architecture/http3.md`;
- `docs/timeout-reference.md`;
- `README.md`;
- `plans/ROADMAP.md`;
- other plan/status/capability documents found by repository-wide search.

Do not modify Cargo dependencies unless an existing build/test issue makes it unavoidable and separately justified.

## Completion criteria

Plan 194 is IMPLEMENTED only when all of the following are true:

- [ ] H3 waits for the next meaningful `ResponseStream` item under an absolute no-progress deadline derived from `response_write_timeout`.
- [ ] Empty chunks do not reset that deadline.
- [ ] Successful non-empty production/send re-arms the next producer budget.
- [ ] Existing H3 send-response/send-data/finish timeouts remain intact.
- [ ] Producer timeout after commitment performs stream/request failure handling without a second response.
- [ ] A timed-out H3 response producer does not cancel healthy sibling streams.
- [ ] Stalled-before-first-byte and stalled-after-progress regression tests exist.
- [ ] Slow-but-progressing stream regression proves total duration may exceed one timeout interval.
- [ ] Empty-chunk regression proves no fake progress.
- [ ] Resource/lifecycle cleanup is verified.
- [ ] H1/H2 behavior and tests remain unchanged/green.
- [ ] Plan 193 status says promotion qualification was preflight-blocked rather than executed despite an unmet prerequisite.
- [ ] Plan 193's release record preserves its evidence inventory while accurately describing it as preflight/gate probing.
- [ ] Live references use consistent Plan 193 wording.
- [ ] Plan 192/timeout documentation no longer claims the baseline unbounded `next().await` was already covered by `response_write_timeout`.
- [ ] H3 remains experimental and upstream promotion blockers remain explicit.
- [ ] Full deterministic/MSRV/supply-chain matrix passes.

## Handoff to Plan 195

After implementation, execute Plan 195 against the exact Plan 194 candidate. Plan 195 owns the corrective evidence record and final documentation consistency check. Do not mark this line closed based only on code review.
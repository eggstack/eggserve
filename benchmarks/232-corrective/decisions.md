# Plan 232 decisions

| Question | Decision | Evidence |
|---|---|---|
| File-read authority | Keep the explicit logical target; never infer it from `BytesMut::capacity()` | Forced-over-capacity adapter regression and full/range tests |
| Default chunk | Keep 128 KiB | `results.json`: live 1 MiB and 16 MiB comparisons; bounded RSS tradeoff |
| TLS evidence | Complete and retain | `results.json`: established keep-alive 1 KiB/1 MiB and 48-connection handshake churn, three trials |
| Plan 231 omitted families | Inherit rather than rerun | No Python facade or admission code path changed; Plan 170 and routine correctness CI remain the evidence |

The 128 KiB choice is profile-specific. It is not an edge-server, TLS-stack,
or universal superiority claim. The higher peak RSS at concurrency 64 is
documented and remains bounded by the runtime's `max_file_streams *
stream_chunk_size` authority.


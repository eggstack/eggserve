# eggserve-server

`eggserve-server` is the Plan 211 generic runtime layer. It owns the
application `Service` contract, TCP listener, bounded connection admission,
HTTP/1 transport conversion, response framing, and request timeout boundary.
It depends on `eggserve-primitives` and transport dependencies.

It has no dependency on `eggserve-core` or `eggserve-static`, so a downstream
application server can select the runtime without inheriting static-file
confinement or MIME implementation code. The direct adapter currently serves
bounded HTTP/1; its optional HTTP/2/TLS dependency edges are reserved for the
next extraction step. HTTP/3 and the mature H2/TLS implementations remain
owned by the compatibility runtime until separately scoped follow-up work
moves them.

The existing `eggserve-core::server` remains the mature 0.1 compatibility
surface. The direct crate is the migration boundary, not a claim that the
experimental compatibility runtime has been promoted.

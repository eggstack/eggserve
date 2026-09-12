# eggserve-server

`eggserve-server` is the Plan 214 generic runtime layer. It owns the
application `Service` contract, TCP listener, bounded connection admission,
HTTP/1 transport conversion, one-shot streaming request bodies, canonical
response normalization, opened-file response streaming, and request timeout
boundary. It depends on `eggserve-primitives` and transport dependencies.

It has no dependency on `eggserve-core` or `eggserve-static`, so a downstream
application server can select the runtime without inheriting static-file
confinement or MIME implementation code. The current direct runtime is
intentionally HTTP/1-shaped; its `http2` and `tls` feature edges compile for
downstream layering, while mature H2/H3, listener-adoption, proxy, and
advanced TLS identity paths remain compatibility features pending extraction.

`eggserve-core::server` remains the 0.1 compatibility surface for those
advanced paths. The direct crate is the preferred generic H1 substrate and is
not a promotion of H2/H3 or tunnel functionality.

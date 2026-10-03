# xmip-core-transport-named-pipe

Named pipe transport: one connection to a named pipe is one Stream — the Windows object under \\.\pipe, a FIFO on Unix. A technology of [xmip-core-transport](https://github.com/IlleNilsson/xmip-core-transport).

A Receive Location waits for a writer within its timeout through the capability's one bounded accept (`transport::socket::accept_within`). A Windows pipe instance has no readiness a safe call can wait on, so the wait there naps a quarter of a millisecond between tries; until 2026-09-27 this crate carried its own copy of the loop with a two-millisecond nap.

A Receive Location keeps its pipe, made on the first receive and kept (`transport::kept::Kept`): a writer that opens it between two receives waits for the next, where until 2026-09-27 each receive made the pipe anew.

A send target is read by `net::Target` in [xmip-core-library-net](https://github.com/IlleNilsson/xmip-core-library-net), the one reading of a URI every technology calls. Until 2026-09-28 this technology stripped its scheme by hand.

## Acknowledgement

Acceptance is at-most-once here. The writer's close ends the Stream and a pipe
carries no reply, so the writer is gone before the receive cycle ends and
nobody is told how it ended. The body is the connection, read to its end as the
runtime asks, never whole in memory.

## Toolchain

`rust-toolchain.toml` pins the toolchain for the whole estate. Do not change it
here.

## Verification

The included workflow is manual-only and calls the versioned shared workflow at
`IlleNilsson/.github@v1`.

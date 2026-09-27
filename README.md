# xmip-core-transport-named-pipe

Named pipe transport: one connection to a named pipe is one Stream — the Windows object under \\.\pipe, a FIFO on Unix. A technology of [xmip-core-transport](https://github.com/IlleNilsson/xmip-core-transport).

A Receive Location waits for a writer within its timeout through the capability's one bounded accept (`transport::socket::accept_within`). A Windows pipe instance has no readiness a safe call can wait on, so the wait there naps a quarter of a millisecond between tries; until 2026-09-27 this crate carried its own copy of the loop with a two-millisecond nap.

A Receive Location keeps its pipe, made on the first receive and kept (`transport::kept::Kept`): a writer that opens it between two receives waits for the next, where until 2026-09-27 each receive made the pipe anew.

## Toolchain

`rust-toolchain.toml` pins the toolchain for the whole estate. Do not change it
here.

## Verification

The included workflow is manual-only and calls the versioned shared workflow at
`IlleNilsson/.github@v1`.

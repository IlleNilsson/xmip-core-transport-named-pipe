# xmip-core-transport-named-pipe

Named pipe transport: one connection to a named pipe is one Stream — the Windows object under \\.\pipe, a FIFO on Unix. A technology of [xmip-core-transport](https://github.com/IlleNilsson/xmip-core-transport).

The pipe is `net::pipe`'s in [xmip-core-library-net](https://github.com/IlleNilsson/xmip-core-library-net), the estate's one, which a node's local operate endpoint makes its pipe through too: one writer is exactly one connection, and an accept returns only once a writer has connected. On Windows it is its owner's, the system's and the administrators', refuses clients from another machine, waits for a writer on the system's event, and names the writer's process (`named-pipe.client-process-id`); on Unix it is a FIFO, its owner's alone. A Receive Location waits for a writer within its timeout. Until 2026-10-21 this crate made its pipe through the `interprocess` crate, whose defects were reported upstream (#100, #101), and on Windows napped a quarter of a millisecond between tries for want of a wait; until 2026-09-27 it carried its own copy of the loop with a two-millisecond nap.

A writer that writes nothing is a Stream of nothing on both systems. Until 2026-10-21 a Windows pipe dropped a writer that wrote nothing and closed before its reader was waiting, and the loopback refused an empty Stream there.

A Receive Location keeps its pipe, made on the first receive and kept (`transport::kept::Kept`): a writer that opens it between two receives waits for the next, where until 2026-09-27 each receive made the pipe anew.

A send target is read by `net::Target` in [xmip-core-library-net](https://github.com/IlleNilsson/xmip-core-library-net), the one reading of a URI every technology calls. Until 2026-09-28 this technology stripped its scheme by hand.

## Acknowledgement

Acceptance is at-most-once here. The writer's close ends the Stream and a pipe
carries no reply, so the writer is gone before the receive cycle ends and
nobody is told how it ended. The body is the connection, read to its end as the
runtime asks, never whole in memory.

A send writes the Message to the pipe as it is read from the Ledger, a piece at
a time, never whole in memory (`Sending::Streamed`, 2026-10-19).

## Toolchain

`rust-toolchain.toml` pins the toolchain for the whole estate. Do not change it
here.

## Verification

The included workflow is manual-only and calls the versioned shared workflow at
`IlleNilsson/.github@v1`.

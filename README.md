# xmip-core-transport-named-pipe

Named pipe transport: one connection to a named pipe is one Stream — the Windows object under \\.\pipe, a FIFO on Unix. A technology of [xmip-core-transport](https://github.com/IlleNilsson/xmip-core-transport).

The pipe is `net::pipe`'s in [xmip-core-library-net](https://github.com/IlleNilsson/xmip-core-library-net), the estate's one, which a node's local operate endpoint makes its pipe through too: one writer is exactly one connection, and an accept returns only once a writer has connected. On Windows it is its owner's, the system's and the administrators', refuses clients from another machine, waits for a writer on the system's event, and names the writer's process (`named-pipe.client-process-id`); on Unix it is a FIFO, its owner's alone. A Receive Location waits for a writer within its timeout. Until 2026-10-21 this crate made its pipe through the `interprocess` crate, whose defects were reported upstream (#100, #101), and on Windows napped a quarter of a millisecond between tries for want of a wait; until 2026-09-27 it carried its own copy of the loop with a two-millisecond nap.

A writer that writes nothing is a Stream of nothing on both systems. Until 2026-10-21 a Windows pipe dropped a writer that wrote nothing and closed before its reader was waiting, and the loopback refused an empty Stream there.

On Unix the FIFO is made on a path claimed through `net::socket_file`, the one claim of a path a listener binds (ADR-0018, amendment 2026-11-19): `<path>.lock` held while the pipe is made, and removed with it; another Location holding the path, or a file of another kind there, which is kept, refuses the Location at phase 9 naming the path; a FIFO a stopped one left is made anew, its owner's alone; and only the Location's own FIFO goes with it. Until 2026-11-19 a FIFO a previous run left was used as it was, and an ordinary file at the path was taken for one and removed as the Location went.

A Receive Location keeps its pipe, made as its transport is opened (`Configured::listening`; on its first receive until 2026-11-16) and kept (`transport::kept::Kept`): a writer that opens it between two receives waits for the next, where until 2026-09-27 each receive made the pipe anew.

A send target is read by `net::Target` in [xmip-core-library-net](https://github.com/IlleNilsson/xmip-core-library-net), the one reading of a URI every technology calls. Until 2026-09-28 this technology stripped its scheme by hand.

## Acknowledgement

Acceptance is at-most-once here. The writer's close ends the Stream and a pipe
carries no reply, so the writer is gone before the receive cycle ends and
nobody is told how it ended. The body is the connection, read to its end as the
runtime asks, never whole in memory.

A send writes the Message to the pipe as it is read from the Ledger, a piece at
a time, never whole in memory (`Sending::Streamed`, 2026-10-19).

## The declaration

What a Location of this technology is configured with is two `const`s in its
`declaration` module, `src/declaration.rs`, beside the code, which
`Configured` reads (ADR-0064, amendment 2026-11-26):
`declaration::SETTINGS`, `timeout`, and `SCHEMES`, `pipe`. It is the same on
every operating system, the Windows pipe and the Unix FIFO alike. Built with
its `declaration` feature alone (`--no-default-features --features
declaration`), the crate is those and nothing more: its code and every crate
the code is built on are behind the default feature. So the library the
surfaces load can carry it without the code, for the language server and the
desktop to suggest and check a named pipe Location's settings though no node
in their process loaded the technology, once the gathering crate of open
problem 48 links it. `test/Declaration.Test.ps1` in the estate holds that
the declaration builds nothing the transport capability does not.

## Toolchain

`rust-toolchain.toml` pins the toolchain for the whole estate. Do not change it
here.

## Verification

The included workflow is manual-only and calls the versioned shared workflow at
`IlleNilsson/.github@v1`.

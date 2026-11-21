# Network egress

tekmor opens no network connection. No source file uses a network client; `tests/egress_guard.rs` fails if one is added without being listed here.

`verify -- <cmd>` runs the allowlisted command on your machine. If that command uses the network (for example `npm test` fetching packages), that traffic belongs to the command, not to tekmor.

To check the statement yourself:

1. Run `cargo test -p tekmor --test egress_guard`.
2. Run `lsof -i -P -n -p <pid>` against a running `tekmor stop-gate` or `tekmor bash-gate`. Neither opens a network socket.

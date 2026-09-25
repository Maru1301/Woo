# ADR 0001: System Git through a Rust process boundary

Status: accepted for M1.

Use the installed Git CLI for compatibility with user configuration, credentials, hooks, LFS, and signing. A Rust runner invokes Git without a shell and returns process metadata. Repository-specific parsing stays in Rust. React receives structured data through Tauri commands. This keeps process behavior in one place and allows later scheduling, streaming, and cancellation work without exposing raw Git output to the UI.

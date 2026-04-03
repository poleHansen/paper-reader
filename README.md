# paper-reader

A Windows desktop paper reading assistant built with Tauri, React, TypeScript, Rust, SQLite, and a Python parsing sidecar.

## Structure

- `app/`: React + TypeScript frontend
- `src-tauri/`: Rust backend, Tauri commands, SQLite, providers
- `code-docs/`: reference-only product and engineering documentation

## Status

This repository now contains the real runtime scaffold at the repository root. The prototype under `code-docs/pre-product/` is not used by the runtime build.

## Tooling

- Frontend package manager: `pnpm`
- Backend: `cargo`

## Prerequisites

- Node.js 20+
- `pnpm` via Corepack
- Rust toolchain with `cargo` on `PATH`

## Common commands

```bash
pnpm install
pnpm dev
pnpm tauri:dev
cargo test --manifest-path src-tauri/Cargo.toml
```

`pnpm dev` only starts the frontend Vite server.

`pnpm tauri:dev` starts a separate frontend dev server on port `1421` and then launches the Tauri desktop app.

## Parse Verification

Generate a local text-based PDF fixture:

```bash
.venv\Scripts\python.exe src-tauri\python-sidecar\generate_sample_pdf.py
```

Then run the desktop app with the workspace Python interpreter configured for the sidecar:

```bash
$env:PAPER_READER_PYTHON = (Resolve-Path .\.venv\Scripts\python.exe)
pnpm tauri:dev
```

In the app, use Upload -> Choose PDF and select `src-tauri/python-sidecar/fixtures/sample-paper.pdf`. After import, confirm metadata to trigger parsing. A successful run should move parse status to `succeeded` and create a parsed JSON artifact under the app data directory.

# paper-reader

A paper reading assistant

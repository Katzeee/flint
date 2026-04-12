# Project Guidelines

## Architecture

- **Backend (server) must be stateless.** It may be restarted at any time and must resume working from persisted state. Do not store runtime counters, caches, or mutable state in memory that would be lost on restart. All durable state lives in workflow files on disk.

## Testing

- Run tests via: `.venv/Scripts/pytest tests/ -v`
- Do not run Python directly; use pytest via venv path or read code for verification.

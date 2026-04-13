# Project Guidelines

## Architecture

- **Backend (server) must be stateless.** It may be restarted at any time and must resume working from persisted state. Do not store runtime counters, caches, or mutable state in memory that would be lost on restart. All durable state lives in workflow files on disk.

## Python Compatibility

- `shared/` and `client/` modules **must remain compatible with Python 3.7**. Do not use syntax or stdlib features introduced after 3.7 (e.g. `str | None` union syntax, `dict` merge operators `|=`, `typing.Literal` without importing from `typing_extensions`, walrus operator `:=`, etc.).
- `server/` may use Python 3.10+ features.

## Testing

- Run tests via: `.venv/Scripts/pytest tests/ -v`
- Do not run Python directly; use pytest via venv path or read code for verification.

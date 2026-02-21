# Project Rules

- The Python project lives in the `python/` subdirectory. Run all `uv` and script commands from there.
- Use `uv` for all dependency and environment management. Never use bare `python -m venv` or `pip`.
  - Install/sync dependencies: `uv sync`
  - Install dev dependencies too: `uv sync --group dev`
  - Add a package: `uv add <package>`
  - Add a dev package: `uv add --group dev <package>`
  - Run a script: `uv run python <script.py>`
- Run scripts from `python/` (`./run_server.sh`, `./run_client.sh`).

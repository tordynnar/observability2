# Project Rules

- Use `uv` for all venv and pip operations. Never use bare `python -m venv` or `pip`.
  - Create venv: `uv venv`
  - Install packages: `uv pip install -r requirements.txt`
  - Add a package: `uv pip install <package>`
- The local venv lives at `.venv/` in the project root. Scripts reference it as `.venv/bin/python`.
- Run scripts from the project root (`./run_server.sh`, `./run_client.sh`).

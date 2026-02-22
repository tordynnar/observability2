# otel-grpc skill

A Claude Code skill for adding OpenTelemetry distributed tracing and correlated logs to gRPC services in Python, Go, and Rust.

## Installation

Copy the `otel-grpc-skill/otel-grpc/` directory into your project's `.claude/skills/` directory:

```bash
mkdir -p .claude/skills
cp -r path/to/otel-grpc-skill/otel-grpc .claude/skills/
```

The result should look like:

```
your-project/
└── .claude/skills/
    └── otel-grpc/
        ├── SKILL.md
        └── references/
            ├── python.md
            ├── go.md
            └── rust.md
```

To install for all your projects instead of just one, copy it to `~/.claude/skills/` instead.

# API Reference

TestForge exposes a REST API for programmatic access from CI/CD pipelines,
editor extensions, and custom integrations. The API server is implemented
in Rust using Axum (Phase 4 of the roadmap).

## Starting the Server

```bash
testforge serve
testforge serve --host 0.0.0.0 --port 8080
```

The server binds to `127.0.0.1:7654` by default (configurable in
`.testforge/config.toml` under `[server]`).

## Base URL

```
http://localhost:7654/api
```

## Authentication

The API currently has no authentication (it binds to localhost by default).
When exposed to a network, configure a reverse proxy with authentication.

## Common Response Format

All endpoints return JSON. Error responses follow this structure:

```json
{
  "error": {
    "code": "INDEX_NOT_READY",
    "message": "Index not initialized. Run `testforge index` first.",
    "suggestion": "Run `testforge index .` to build the search index."
  }
}
```

## Endpoints

---

### Health Check

Check if the server is running and the index is available.

```
GET /api/health
```

**Response** `200 OK`

```json
{
  "status": "healthy",
  "version": "0.1.0",
  "index_ready": true,
  "uptime_seconds": 3600
}
```

---

### Index Status

Get statistics about the current index.

```
GET /api/status
```

**Response** `200 OK`

```json
{
  "file_count": 147,
  "symbol_count": 892,
  "embedding_count": 892,
  "languages": ["python", "javascript"],
  "last_indexed": "2025-01-15T10:30:00Z",
  "watcher_active": false
}
```

---

### Trigger Indexing

Start a full or incremental re-index of the project.

```
POST /api/index
```

**Request Body**

```json
{
  "path": ".",
  "clean": false
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `path` | string | `"."` | Directory to index (relative to project root) |
| `clean` | boolean | `false` | Clear existing index before re-indexing |

**Response** `202 Accepted`

```json
{
  "job_id": "idx_a1b2c3d4",
  "status": "running",
  "progress_ws": "ws://localhost:7654/ws/progress/idx_a1b2c3d4"
}
```

**Progress via WebSocket**

Connect to the WebSocket URL to receive real-time progress:

```json
{"type": "progress", "files_done": 42, "files_total": 147, "current_file": "src/auth.py"}
{"type": "complete", "files_indexed": 147, "symbols_extracted": 892, "duration_ms": 823}
```

---

### Search

Search the codebase using keywords or natural language.

```
POST /api/search
```

**Request Body**

```json
{
  "query": "payment validation logic",
  "limit": 10,
  "filters": {
    "languages": ["python"],
    "kinds": ["function", "method"],
    "paths": ["src/"],
    "visibility": "public"
  }
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `query` | string | *required* | Search query (keywords or natural language) |
| `limit` | integer | `10` | Maximum results (1–100) |
| `filters.languages` | string[] | all | Filter by programming language |
| `filters.kinds` | string[] | all | Filter by symbol kind |
| `filters.paths` | string[] | all | Filter by file path prefix |
| `filters.visibility` | string | all | Filter by visibility (`public`, `private`) |

**Response** `200 OK`

```json
{
  "results": [
    {
      "symbol": {
        "name": "validate_payment",
        "qualified_name": "PaymentService.validate_payment",
        "kind": "method",
        "language": "python",
        "file_path": "src/payments/service.py",
        "start_line": 45,
        "end_line": 78,
        "signature": "def validate_payment(self, amount: Decimal, currency: str) -> ValidationResult",
        "docstring": "Validate a payment amount and currency against business rules.",
        "dependencies": ["check_currency", "check_amount_limits"],
        "parent": "PaymentService",
        "visibility": "public"
      },
      "score": 0.94,
      "match_source": "hybrid"
    }
  ],
  "total_results": 1,
  "search_time_ms": 12
}
```

---

### Generate Tests

Generate tests for a specific symbol or file.

```
POST /api/generate-tests
```

**Request Body**

```json
{
  "target": "PaymentService.validate_payment",
  "framework": "pytest",
  "include_edge_cases": true,
  "include_mocks": true,
  "max_tokens": 4096,
  "temperature": 0.2
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `target` | string | *required* | Qualified symbol name or file path |
| `framework` | string | from config | Test framework to use |
| `include_edge_cases` | boolean | `true` | Analyze and test edge cases |
| `include_mocks` | boolean | `true` | Generate mock setups |
| `max_tokens` | integer | `4096` | Max LLM response tokens |
| `temperature` | float | `0.2` | LLM sampling temperature |

**Response** `202 Accepted`

```json
{
  "job_id": "gen_e5f6g7h8",
  "status": "running",
  "target": "PaymentService.validate_payment"
}
```

### Get Generation Result

```
GET /api/generate-tests/{job_id}
```

**Response** `200 OK` (when complete)

```json
{
  "job_id": "gen_e5f6g7h8",
  "status": "complete",
  "result": {
    "source": "import pytest\nfrom unittest.mock import ...\n\ndef test_validate_payment_valid_amount():\n    ...",
    "file_name": "test_service.py",
    "target_symbol": "PaymentService.validate_payment",
    "test_count": 8,
    "framework": "pytest"
  }
}
```

**Response** `200 OK` (still running)

```json
{
  "job_id": "gen_e5f6g7h8",
  "status": "running",
  "progress": "Generating tests..."
}
```

---

### List Symbols

List all indexed symbols with optional filtering.

```
GET /api/symbols
```

**Query Parameters**

| Parameter | Type | Description |
|-----------|------|-------------|
| `file` | string | Filter by file path |
| `kind` | string | Filter by symbol kind |
| `language` | string | Filter by language |
| `limit` | integer | Max results (default 100) |
| `offset` | integer | Pagination offset |

**Response** `200 OK`

```json
{
  "symbols": [...],
  "total": 892,
  "limit": 100,
  "offset": 0
}
```

---

### Get Symbol Details

Get full details for a specific symbol.

```
GET /api/symbols/{qualified_name}
```

**Response** `200 OK`

```json
{
  "symbol": {
    "name": "validate_payment",
    "qualified_name": "PaymentService.validate_payment",
    "kind": "method",
    "source": "def validate_payment(self, ...):\n    ...",
    ...
  },
  "context": {
    "dependencies": [...],
    "callers": [...],
    "siblings": [...]
  }
}
```

---

## WebSocket API

### Progress Stream

```
WS /ws/progress/{job_id}
```

Streams real-time progress updates for long-running operations
(indexing, test generation). Messages are JSON-encoded:

```json
{"type": "progress", "message": "Parsing src/auth.py...", "percent": 45}
{"type": "complete", "result": {...}}
{"type": "error", "message": "Failed to parse src/broken.py"}
```

## Error Codes

| Code | HTTP Status | Description |
|------|-------------|-------------|
| `INDEX_NOT_READY` | 503 | Index not built yet |
| `SYMBOL_NOT_FOUND` | 404 | Requested symbol not in index |
| `EMPTY_QUERY` | 400 | Search query is empty |
| `INVALID_CONFIG` | 400 | Configuration error |
| `LLM_ERROR` | 502 | LLM provider returned an error |
| `RATE_LIMITED` | 429 | Too many requests |
| `INTERNAL` | 500 | Unexpected internal error |

## Rate Limits

The built-in server has no rate limits. When deploying behind a reverse
proxy, configure rate limiting there. Recommended limits:

- Search: 60 requests/minute
- Generate tests: 10 requests/minute
- Index: 1 request/minute

## Client Libraries

### Python

```python
from testforge_ai.bridge import TestForgeBridge

bridge = TestForgeBridge(Path("."))
symbols = bridge.search_symbols("authentication")
```

### curl

```bash
# Search
curl -X POST http://localhost:7654/api/search \
  -H "Content-Type: application/json" \
  -d '{"query": "authentication", "limit": 5}'

# Trigger indexing
curl -X POST http://localhost:7654/api/index \
  -H "Content-Type: application/json" \
  -d '{"clean": false}'

# Generate tests
curl -X POST http://localhost:7654/api/generate-tests \
  -H "Content-Type: application/json" \
  -d '{"target": "AuthService.authenticate"}'
```

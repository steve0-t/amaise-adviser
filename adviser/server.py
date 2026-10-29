#!/usr/bin/env python3
"""Local AMEISE observer and adviser. This process never contacts the simulator.

Run ``python3 adviser/server.py`` and open http://127.0.0.1:8765.
Only Python's standard library is required. State is persisted in SQLite.
"""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import ipaddress
import json
import math
import mimetypes
from pathlib import Path
import re
import socket
import sqlite3
import sys
import threading
from typing import Any
from urllib.parse import parse_qs, unquote, urlsplit


PROJECT_ROOT = Path(__file__).resolve().parent.parent
MAX_BODY_BYTES = 1024 * 1024
MAX_MISSING_SEQUENCES = 1000
LOOPBACK_HOSTS = {"127.0.0.1", "localhost", "::1"}
DOCUMENTS = {
    "README.md", "AMAISE_commandlist-qa200-en-v1.95.pdf",
    "json-api-integration.md", "adviser-command-policy.json",
    "adviser-event.schema.json", "adviser-advice.schema.json",
    "adviser-events.jsonl", "adviser-demo-advice.json",
}


class ApiError(Exception):
    def __init__(self, status: int, code: str, message: str):
        super().__init__(message)
        self.status, self.code, self.message = status, code, message


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="microseconds").replace("+00:00", "Z")


def canonical(value: Any) -> str:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False)


def _unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"Duplicate JSON field: {key}")
        result[key] = value
    return result


def parse_json(data: bytes | str) -> Any:
    def invalid_constant(value: str) -> None:
        raise ValueError(f"Invalid JSON number: {value}")

    try:
        if isinstance(data, bytes):
            data = data.decode("utf-8")
        return json.loads(data, object_pairs_hook=_unique_object, parse_constant=invalid_constant)
    except (ValueError, UnicodeError, RecursionError) as exc:
        raise ApiError(400, "invalid_json", "Body must be valid UTF-8 JSON without duplicate fields.") from exc


def validate(value: Any, schema: dict, root: dict | None = None, path: str = "$") -> None:
    """Validate the JSON Schema vocabulary used by the two bundled v2 schemas.

    This is intentionally a small, dependency-free validator, not a general JSON
    Schema implementation. Schemas are local trusted files, never request inputs.
    """
    root = root or schema

    def reject(message: str) -> None:
        raise ApiError(400, "validation_error", f"{path}: {message}")

    if schema is False:
        reject("value is not permitted")
    if schema is True:
        return
    if "$ref" in schema:
        ref = schema["$ref"]
        if not ref.startswith("#/"):
            raise RuntimeError("Only local schema references are supported")
        target = root
        for part in ref[2:].split("/"):
            target = target[part.replace("~1", "/").replace("~0", "~")]
        validate(value, target, root, path)
    for branch in schema.get("allOf", []):
        validate(value, branch, root, path)
    for keyword in ("oneOf", "anyOf"):
        if keyword in schema:
            matches = 0
            for branch in schema[keyword]:
                try:
                    validate(value, branch, root, path)
                    matches += 1
                except ApiError:
                    pass
            if not matches or (keyword == "oneOf" and matches != 1):
                reject(f"does not match the required {keyword} variant")
    if "const" in schema and (type(value) is not type(schema["const"]) or value != schema["const"]):
        reject("unexpected constant value")
    if "enum" in schema and not any(type(value) is type(item) and value == item for item in schema["enum"]):
        reject("value is not in the permitted enumeration")
    checks = {
        "null": lambda: value is None,
        "boolean": lambda: type(value) is bool,
        "integer": lambda: type(value) is int,
        "number": lambda: type(value) is int or (type(value) is float and math.isfinite(value)),
        "string": lambda: type(value) is str,
        "array": lambda: type(value) is list,
        "object": lambda: type(value) is dict,
    }
    types = schema.get("type")
    if types:
        types = types if isinstance(types, list) else [types]
        if not any(checks[expected]() for expected in types):
            reject(f"expected {' or '.join(types)}")
    if type(value) is dict:
        if len(value) < schema.get("minProperties", 0) or len(value) > schema.get("maxProperties", math.inf):
            reject("incorrect number of fields")
        missing = set(schema.get("required", [])) - value.keys()
        if missing:
            reject(f"missing required fields: {', '.join(sorted(missing))}")
        properties = schema.get("properties", {})
        extra = value.keys() - properties.keys()
        if extra and schema.get("additionalProperties", True) is False:
            reject(f"unknown fields: {', '.join(sorted(extra))}")
        for key, item in value.items():
            child_schema = properties.get(key, schema.get("additionalProperties", {}))
            if isinstance(child_schema, (dict, bool)):
                validate(item, child_schema, root, f"{path}.{key}")
    if type(value) is list:
        if len(value) < schema.get("minItems", 0) or len(value) > schema.get("maxItems", math.inf):
            reject("incorrect number of items")
        if schema.get("uniqueItems") and len({canonical(item) for item in value}) != len(value):
            reject("items must be unique")
        for index, item in enumerate(value):
            if "items" in schema:
                validate(item, schema["items"], root, f"{path}[{index}]")
    if type(value) is str:
        if len(value) < schema.get("minLength", 0) or len(value) > schema.get("maxLength", math.inf):
            reject("incorrect string length")
        if "pattern" in schema and re.search(schema["pattern"], value) is None:
            reject("string does not match the required pattern")
        if schema.get("format") == "date-time":
            try:
                # Require a full date/time and a zone rather than accepting a date.
                if not re.fullmatch(r"\d{4}-\d{2}-\d{2}[Tt]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:[Zz]|[+-]\d{2}:\d{2})", value):
                    raise ValueError("timestamp")
                parsed = datetime.fromisoformat(value.upper().replace("Z", "+00:00"))
                if parsed.tzinfo is None:
                    raise ValueError("timezone")
            except ValueError:
                reject("expected an RFC 3339 date-time with timezone")
    if type(value) in (int, float):
        if type(value) is float and not math.isfinite(value):
            reject("number must be finite")
        if type(value) is int and not -(2 ** 63) <= value < 2 ** 63:
            reject("integer exceeds the supported signed 64-bit range")
        if value < schema.get("minimum", -math.inf) or value > schema.get("maximum", math.inf):
            reject("number is outside the permitted range")


def quality_for(events: list[dict]) -> dict:
    """Rebuild state in source sequence order, including late-arriving packets."""
    missing: list[int] = []
    missing_count = 0
    expected = 1
    requests: dict[str, dict] = {}
    for event in events:
        sequence = event["sequence"]
        if sequence > expected:
            count = sequence - expected
            missing_count += count
            room = MAX_MISSING_SEQUENCES - len(missing)
            missing.extend(range(expected, expected + min(count, room)))
        expected = sequence + 1
        if event["kind"] == "game.selected":
            continue
        payload = event["payload"]
        request = requests.setdefault(payload["request_id"], {"commands": set(), "pending": False})
        request["commands"].add(payload["command"])
        request["pending"] = event["kind"] != "command.result"
    uncertain = bool(missing_count) or any(
        request["pending"] or len(request["commands"]) != 1 for request in requests.values()
    )
    return {"missing_sequences": missing, "uncertain": uncertain,
            "truncated": missing_count > MAX_MISSING_SEQUENCES}


class AdviserStore:
    def __init__(self, data_dir: Path, project_root: Path = PROJECT_ROOT):
        self.root = project_root
        docs = project_root / "docs"
        self.event_schema = json.loads((docs / "adviser-event.schema.json").read_text())
        self.advice_schema = json.loads((docs / "adviser-advice.schema.json").read_text())
        self.policy = json.loads((docs / "adviser-command-policy.json").read_text())
        self.commands = {command["id"]: command for command in self.policy["commands"]}
        data_dir = Path(data_dir)
        data_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
        self.lock = threading.RLock()
        self.db = sqlite3.connect(data_dir / "adviser.sqlite3", check_same_thread=False)
        self.db.row_factory = sqlite3.Row
        self.db.execute("PRAGMA journal_mode=WAL")
        self.db.executescript("""
            CREATE TABLE IF NOT EXISTS streams (
                stream_id TEXT PRIMARY KEY, game TEXT NOT NULL,
                revision INTEGER NOT NULL, last_seen_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS events (
                event_id TEXT PRIMARY KEY, stream_id TEXT NOT NULL,
                sequence INTEGER NOT NULL, document TEXT NOT NULL,
                UNIQUE(stream_id, sequence)
            );
            CREATE TABLE IF NOT EXISTS advice (
                advice_id TEXT PRIMARY KEY, stream_id TEXT NOT NULL,
                document TEXT NOT NULL, created_at TEXT NOT NULL
            );
        """)
        self.db.commit()

    def close(self) -> None:
        with self.lock:
            self.db.close()

    def ingest(self, event: Any) -> tuple[int, dict]:
        validate(event, self.event_schema)
        encoded = canonical(event)
        with self.lock, self.db:
            # Serialize writers across processes as well as HTTP worker threads.
            self.db.execute("BEGIN IMMEDIATE")
            duplicate = self.db.execute("SELECT document FROM events WHERE event_id=?", (event["event_id"],)).fetchone()
            stream = self.db.execute("SELECT * FROM streams WHERE stream_id=?", (event["stream_id"],)).fetchone()
            if duplicate:
                if duplicate["document"] != encoded:
                    raise ApiError(409, "event_conflict", "event_id already exists with different content.")
                return 200, {"accepted": True, "duplicate": True, "stream_id": event["stream_id"], "revision": stream["revision"]}
            if self.db.execute("SELECT 1 FROM events WHERE stream_id=? AND sequence=?", (event["stream_id"], event["sequence"])).fetchone():
                raise ApiError(409, "sequence_conflict", "This stream already contains a different event at this sequence.")
            game = canonical(event["game"])
            if stream and stream["game"] != game:
                raise ApiError(409, "game_conflict", "A stream cannot contain events from different games.")
            revision = stream["revision"] + 1 if stream else 1
            self.db.execute("INSERT INTO events VALUES (?, ?, ?, ?)", (event["event_id"], event["stream_id"], event["sequence"], encoded))
            self.db.execute("INSERT INTO streams VALUES (?, ?, ?, ?) ON CONFLICT(stream_id) DO UPDATE SET revision=excluded.revision,last_seen_at=excluded.last_seen_at", (event["stream_id"], game, revision, utc_now()))
            return 201, {"accepted": True, "duplicate": False, "stream_id": event["stream_id"], "revision": revision}

    def _snapshot(self, stream_id: str | None = None) -> dict:
        streams = []
        for row in self.db.execute("""SELECT s.*, COUNT(e.event_id) AS event_count, MAX(e.sequence) AS last_sequence
                FROM streams s JOIN events e USING(stream_id) GROUP BY s.stream_id
                ORDER BY MAX(e.rowid) DESC"""):
            streams.append({**dict(row), "game": json.loads(row["game"])})
        if stream_id is None and streams:
            stream_id = streams[0]["stream_id"]
        selected = next((stream for stream in streams if stream["stream_id"] == stream_id), None)
        if stream_id is not None and selected is None:
            raise ApiError(404, "stream_not_found", "Unknown stream_id.")
        events = [json.loads(row[0]) for row in self.db.execute("SELECT document FROM events WHERE stream_id=? ORDER BY sequence", (stream_id,))]
        revision = selected["revision"] if selected else 0
        advice = []
        for row in self.db.execute("SELECT document FROM advice WHERE stream_id=? ORDER BY rowid DESC", (stream_id,)):
            record = json.loads(row[0])
            record["stale"] = record["based_on_revision"] != revision or record["policy_version"] != self.policy["policy_version"]
            advice.append(record)
        return {"api_version": "2", "streams": streams, "selected_stream_id": stream_id,
                "revision": revision, "events": events, "advice": advice,
                "policy": self.policy, "quality": quality_for(events)}

    def snapshot(self, stream_id: str | None = None, context: bool = False) -> dict:
        with self.lock, self.db:
            self.db.execute("BEGIN")
            state = self._snapshot(stream_id)
            if context:
                selected = next((stream for stream in state["streams"] if stream["stream_id"] == state["selected_stream_id"]), None)
                state["game"] = selected["game"] if selected else None
                state["turn_id"] = state["events"][-1]["turn_id"] if state["events"] else None
                state["policy_version"] = self.policy["policy_version"]
            return state

    def submit_advice(self, advice: Any) -> tuple[int, dict]:
        validate(advice, self.advice_schema)
        with self.lock, self.db:
            self.db.execute("BEGIN IMMEDIATE")
            state = self._snapshot(advice["stream_id"])
            if advice["based_on_revision"] != state["revision"]:
                raise ApiError(409, "stale_advice", "Advice must use the stream's current ingestion revision.")
            if advice["policy_version"] != self.policy["policy_version"]:
                raise ApiError(409, "policy_conflict", "Advice must use the current command policy version.")
            known_ids = {event["event_id"] for event in state["events"]}
            for item in advice["observations"] + advice["recommendations"]:
                if not set(item["evidence_event_ids"]).issubset(known_ids):
                    raise ApiError(400, "invalid_evidence", "Every evidence_event_id must belong to the selected stream.")
            rendered = []
            for recommendation in advice["recommendations"]:
                command = self.commands.get(recommendation["command_id"])
                if command is None or command["recommendation"] != "allowed":
                    raise ApiError(400, "command_not_allowed", "This command is not allowed as an adviser recommendation.")
                arguments = recommendation["arguments"]
                parameters = command["parameters"]
                if set(arguments) != set(parameters):
                    raise ApiError(400, "invalid_arguments", "Arguments must match every policy parameter exactly.")
                for name, parameter in parameters.items():
                    try:
                        validate(arguments[name], parameter, path=f"$.recommendations.arguments.{name}")
                    except ApiError as exc:
                        raise ApiError(400, "invalid_arguments", exc.message) from exc
                    if type(arguments[name]) is str and (not arguments[name].strip() or len(arguments[name]) > 256
                            or any(ord(char) < 32 or ord(char) == 127 for char in arguments[name])):
                        raise ApiError(400, "invalid_arguments", "Text arguments must contain 1–256 printable characters.")
                    if name == "report_name":
                        cited_results = [event for event in state["events"]
                                         if event["event_id"] in recommendation["evidence_event_ids"] and event["kind"] == "command.result"]
                        if not any(arguments[name] in event["payload"][field]
                                   for event in cited_results for field in ("feedback", "kernel_output_raw")):
                            raise ApiError(400, "invalid_arguments", "report_name must occur in the cited result feedback or raw output.")
                reviewers = [value for key, value in arguments.items() if key.startswith("reviewer_")]
                if len(reviewers) != len(set(reviewers)):
                    raise ApiError(400, "invalid_arguments", "Each reviewer must be a different developer.")
                rendered.append({"command_id": command["id"], "command": command["template"].format(**arguments),
                                 "reason": recommendation["reason"], "evidence_event_ids": recommendation["evidence_event_ids"]})
            if rendered and (state["quality"]["uncertain"] or state["quality"]["truncated"]):
                raise ApiError(409, "context_uncertain", "Resolve missing packets, pending commands, callbacks, or transport uncertainty before recommending commands.")
            existing = self.db.execute("SELECT document FROM advice WHERE advice_id=?", (advice["advice_id"],)).fetchone()
            if existing:
                saved = json.loads(existing["document"])
                original = {key: saved[key] for key in advice}
                if canonical(original) != canonical(advice):
                    raise ApiError(409, "advice_conflict", "advice_id already exists with different content.")
                saved["stale"] = False
                return 200, saved
            record = {**advice, "created_at": utc_now(), "stale": False, "rendered_recommendations": rendered}
            self.db.execute("INSERT INTO advice VALUES (?, ?, ?, ?)", (advice["advice_id"], advice["stream_id"], canonical(record), record["created_at"]))
            return 201, record

    def examples(self) -> dict:
        docs = self.root / "docs"
        events = [json.loads(line) for line in (docs / "adviser-events.jsonl").read_text().splitlines() if line.strip()]
        advice = json.loads((docs / "adviser-demo-advice.json").read_text())
        return {"events": events, "advice": advice}


class AdviserHTTPServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, address: tuple[str, int], store: AdviserStore):
        if address[0] not in LOOPBACK_HOSTS:
            raise ValueError("Only a loopback address is permitted")
        if ":" in address[0]:
            self.address_family = socket.AF_INET6
        self.store = store
        super().__init__(address, AdviserHandler)


class AdviserHandler(BaseHTTPRequestHandler):
    server_version = "AMEISEAdviser/2"
    sys_version = ""

    def setup(self) -> None:
        super().setup()
        self.connection.settimeout(10)

    def log_message(self, format: str, *args: Any) -> None:
        # Do not log commands, raw packets, or query strings containing game IDs.
        sys.stderr.write(f"{self.log_date_time_string()} {self.command} {args[1] if len(args) > 1 else ''}\n")

    def _check_local_request(self) -> None:
        try:
            address = ipaddress.ip_address(self.client_address[0])
            if not address.is_loopback:
                raise ValueError("not loopback")
            host = urlsplit("//" + self.headers.get("Host", ""))
            if host.hostname not in LOOPBACK_HOSTS or (host.port or 80) != self.server.server_address[1] or host.username or host.password or host.path or host.query or host.fragment:
                raise ValueError("host")
        except ValueError as exc:
            raise ApiError(400, "invalid_host", "Use the local service's loopback host and port.") from exc
        origin = self.headers.get("Origin")
        if origin is not None:
            try:
                parsed = urlsplit(origin)
                if parsed.scheme != "http" or parsed.hostname not in LOOPBACK_HOSTS or (parsed.port or 80) != self.server.server_address[1] or parsed.path or parsed.query or parsed.fragment or parsed.username or parsed.password:
                    raise ValueError("origin")
            except ValueError as exc:
                raise ApiError(403, "foreign_origin", "Only this loopback service's browser origin is allowed.") from exc

    def _respond(self, status: int, data: bytes, content_type: str) -> None:
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Referrer-Policy", "no-referrer")
        self.send_header("Content-Security-Policy", "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'")
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(data)

    def _json(self, status: int, value: Any) -> None:
        self._respond(status, canonical(value).encode("utf-8"), "application/json; charset=utf-8")

    def _body(self) -> Any:
        if self.headers.get("Transfer-Encoding"):
            raise ApiError(400, "invalid_body", "Transfer-Encoding is not supported; send Content-Length.")
        if self.headers.get("Content-Type", "").split(";", 1)[0].strip().lower() != "application/json":
            raise ApiError(400, "invalid_content_type", "Use Content-Type: application/json.")
        try:
            length = int(self.headers.get("Content-Length", ""))
        except ValueError as exc:
            raise ApiError(400, "invalid_body", "A valid Content-Length is required.") from exc
        if length < 1:
            raise ApiError(400, "invalid_body", "JSON body is required.")
        if length > MAX_BODY_BYTES:
            raise ApiError(413, "body_too_large", "JSON body exceeds the 1 MiB limit.")
        data = self.rfile.read(length)
        if len(data) != length:
            raise ApiError(400, "invalid_body", "Incomplete request body.")
        return parse_json(data)

    def _route(self) -> None:
        self._check_local_request()
        parsed = urlsplit(self.path)
        path = unquote(parsed.path)
        query = parse_qs(parsed.query, keep_blank_values=True)
        if self.command in ("GET", "HEAD"):
            if path == "/api/v2/health":
                return self._json(200, {"status": "ok", "mode": "adviser", "simulator_control": False})
            if path == "/api/v2/policy":
                return self._json(200, self.server.store.policy)
            if path == "/api/v2/examples":
                return self._json(200, self.server.store.examples())
            if path in ("/api/v2/state", "/api/v2/context"):
                if set(query) - {"stream_id"} or ("stream_id" in query and (len(query["stream_id"]) != 1 or not query["stream_id"][0])):
                    raise ApiError(400, "invalid_query", "Only one nonempty stream_id parameter is supported.")
                stream_id = query.get("stream_id", [None])[0]
                return self._json(200, self.server.store.snapshot(stream_id, context=path.endswith("context")))
            return self._static(path)
        if self.command == "POST":
            if path == "/api/v2/events":
                status, value = self.server.store.ingest(self._body())
                return self._json(status, value)
            if path == "/api/v2/advice":
                status, value = self.server.store.submit_advice(self._body())
                return self._json(status, value)
        raise ApiError(404, "not_found", "No such endpoint. This adviser has no simulator control API.")

    def _static(self, path: str) -> None:
        if path.startswith("/docs/") and path[len("/docs/"):] in DOCUMENTS:
            filename = self.server.store.root / path.lstrip("/")
        elif path in ("/", "/index.html", "/app.js", "/theme.js", "/styles.css"):
            filename = self.server.store.root / "adviser" / "dist" / ("index.html" if path == "/" else path.lstrip("/"))
        else:
            raise ApiError(404, "not_found", "No such resource.")
        if not filename.is_file():
            raise ApiError(404, "not_found", "Resource has not been created.")
        content_type = mimetypes.guess_type(filename.name)[0] or "application/octet-stream"
        if content_type.startswith("text/") or filename.suffix in (".js", ".json", ".jsonl"):
            content_type += "; charset=utf-8"
        self._respond(200, filename.read_bytes(), content_type)

    def _handle(self) -> None:
        try:
            self._route()
        except ApiError as exc:
            self._json(exc.status, {"error": {"code": exc.code, "message": exc.message}})
        except (BrokenPipeError, ConnectionResetError, TimeoutError):
            self.close_connection = True
        except Exception:
            # Avoid exposing filesystem paths or raw packets through HTTP errors.
            self._json(500, {"error": {"code": "internal_error", "message": "The local adviser could not complete the request."}})

    do_GET = do_HEAD = do_POST = do_OPTIONS = do_PUT = do_DELETE = _handle


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", choices=sorted(LOOPBACK_HOSTS), default="127.0.0.1", help="Loopback bind address")
    parser.add_argument("--port", type=int, default=8765)
    parser.add_argument("--data-dir", type=Path, default=Path(__file__).resolve().parent / ".runtime")
    args = parser.parse_args(argv)
    if not 0 <= args.port <= 65535:
        parser.error("--port must be between 0 and 65535")
    store = AdviserStore(args.data_dir)
    server = AdviserHTTPServer((args.host, args.port), store)
    host = f"[{args.host}]" if ":" in args.host else args.host
    print(f"AMEISE adviser: http://{host}:{server.server_address[1]} (observation only)", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
        store.close()


if __name__ == "__main__":
    main()

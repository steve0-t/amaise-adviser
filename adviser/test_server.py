"""Meaningful protocol, persistence, policy and HTTP-boundary tests (stdlib only)."""

from copy import deepcopy
from http.client import HTTPConnection, HTTPResponse
from io import BytesIO
import json
from pathlib import Path
import tempfile
import threading
from types import SimpleNamespace
import unittest

from adviser.server import (AdviserHandler, AdviserHTTPServer, AdviserStore, ApiError,
                            MAX_BODY_BYTES, PROJECT_ROOT, parse_json)


class StoreTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.store = AdviserStore(Path(self.directory.name))
        self.base = self.store.examples()["events"][0]
        self.base.update(event_id="e1", stream_id="stream", source="import")

    def tearDown(self):
        self.store.close()
        self.directory.cleanup()

    def event(self, sequence=1, kind="game.selected", request_id="request", command="show me specification", **updates):
        event = deepcopy(self.base)
        event.update(event_id=f"e{sequence}", sequence=sequence, kind=kind)
        event.update(updates)
        if kind == "command.sent":
            event["payload"] = {"request_id": request_id, "command": command}
        elif kind == "command.result":
            event["payload"] = {"request_id": request_id, "command": command,
                                "feedback": "Observed result", "kernel_output_raw": "",
                                "simulation_date_raw": None}
        elif kind == "transport.uncertain":
            event["payload"] = {"request_id": request_id, "command": command, "message": "Connection lost after send"}
        elif kind == "callback.requested":
            event["payload"] = {"request_id": request_id, "command": command, "question": "Choose an answer",
                                "choices": [{"id": 0, "label": "First"}], "previous_answer_ids": []}
        return event

    def advice(self, **updates):
        state = self.store.snapshot("stream")
        return {"api_version": "2", "advice_id": "a1", "stream_id": "stream",
                "based_on_revision": state["revision"], "policy_version": "qa200-1.95-v1", "source": "agent",
                "summary": "Inspect the observed state", "observations": [{"text": "A game was selected", "evidence_event_ids": ["e1"]}],
                "recommendations": [{"command_id": "show_specification", "arguments": {},
                                     "reason": "Read current information", "evidence_event_ids": ["e1"]}],
                "missing_information": [], **updates}

    def assert_error(self, function, value, code, status=400):
        with self.assertRaises(ApiError) as caught:
            function(value)
        self.assertEqual((caught.exception.status, caught.exception.code), (status, code))

    def test_empty_snapshot_does_not_fabricate_a_game(self):
        state = self.store.snapshot(context=True)
        self.assertEqual(state["revision"], 0)
        self.assertIsNone(state["selected_stream_id"])
        self.assertIsNone(state["game"])
        self.assertEqual(state["events"], [])
        self.assertEqual(state["advice"], [])
        self.assertFalse(state["quality"]["uncertain"])

    def test_bundled_demo_ingests_and_renders_structured_advice(self):
        examples = self.store.examples()
        for event in examples["events"]:
            status, _ = self.store.ingest(event)
            self.assertEqual(status, 201)
        status, record = self.store.submit_advice(examples["advice"])
        self.assertEqual(status, 201)
        self.assertEqual(record["rendered_recommendations"][0]["command"], "show me review report")
        self.assertFalse(record["stale"])

    def test_malformed_events_are_rejected_before_storage(self):
        invalid = []
        event = self.event(); event["credentials"] = {"password": "forbidden"}; invalid.append(event)
        event = self.event(); del event["source"]; invalid.append(event)
        event = self.event(); event["sequence"] = True; invalid.append(event)
        event = self.event(); event["sequence"] = 10 ** 200; invalid.append(event)
        event = self.event(); event["game"]["game_id"] = 0; invalid.append(event)
        event = self.event(); event["occurred_at"] = "2026-02-30T10:00:00Z"; invalid.append(event)
        event = self.event(); event["occurred_at"] = "2026-10-28"; invalid.append(event)
        event = self.event(); event["payload"]["extra"] = "invalid"; invalid.append(event)
        invalid.extend([{}, [], None])
        for event in invalid:
            with self.subTest(event=event):
                self.assert_error(self.store.ingest, event, "validation_error")
        self.assertEqual(self.store.snapshot()["revision"], 0)

    def test_duplicate_is_idempotent_but_changed_packet_and_sequence_conflict(self):
        event = self.event()
        self.store.ingest(event)
        status, record = self.store.ingest(deepcopy(event))
        self.assertEqual(status, 200)
        self.assertTrue(record["duplicate"])
        self.assertEqual(record["revision"], 1)
        changed = deepcopy(event); changed["turn_id"] = 1
        self.assert_error(self.store.ingest, changed, "event_conflict", 409)
        changed = deepcopy(event); changed["event_id"] = "other"
        self.assert_error(self.store.ingest, changed, "sequence_conflict", 409)
        self.assertEqual(self.store.snapshot()["revision"], 1)

    def test_stream_identity_and_cross_stream_event_identity_are_enforced(self):
        self.store.ingest(self.event())
        changed = self.event(2); changed["game"]["game_id"] = 2
        self.assert_error(self.store.ingest, changed, "game_conflict", 409)
        changed = self.event(stream_id="other")
        self.assert_error(self.store.ingest, changed, "event_conflict", 409)

    def test_every_manual_or_excluded_command_is_rejected_as_advice(self):
        self.store.ingest(self.event())
        for command in self.store.policy["commands"]:
            if command["recommendation"] == "allowed":
                continue
            advice = self.advice()
            advice["recommendations"][0]["command_id"] = command["id"]
            with self.subTest(command=command["id"]):
                self.assert_error(self.store.submit_advice, advice, "command_not_allowed")

    def test_observation_of_excluded_and_unknown_commands_is_still_recorded(self):
        self.store.ingest(self.event())
        self.store.ingest(self.event(2, "command.sent", command="finish project"))
        self.store.ingest(self.event(3, "command.result", command="finish project"))
        self.store.ingest(self.event(4, "command.sent", request_id="other", command="unknown upstream command"))
        self.assertEqual(len(self.store.snapshot()["events"]), 4)

    def test_unparsed_callback_is_preserved_and_still_blocks_recommendations(self):
        self.store.ingest(self.event())
        self.store.ingest(self.event(2, "command.sent"))
        callback = self.event(3, "callback.requested")
        callback["payload"].update(choices=[], alternatives_raw="Unparsed upstream alternatives")
        self.store.ingest(callback)
        state = self.store.snapshot("stream")
        self.assertEqual(state["events"][-1]["payload"]["alternatives_raw"], "Unparsed upstream alternatives")
        self.assertTrue(state["quality"]["uncertain"])
        self.assert_error(self.store.submit_advice, self.advice(), "context_uncertain", 409)

    def test_extra_arbitrary_command_text_is_rejected(self):
        self.store.ingest(self.event())
        advice = self.advice()
        advice["recommendations"][0]["command"] = "finish project"
        self.assert_error(self.store.submit_advice, advice, "validation_error")
        advice = self.advice()
        advice["recommendations"][0]["command_id"] = "arbitrary command"
        self.assert_error(self.store.submit_advice, advice, "command_not_allowed")

    def test_fabricated_or_wrong_type_or_extra_arguments_are_rejected(self):
        self.store.ingest(self.event())
        cases = [("show_activity", {"developer": "Fabricated"}),
                 ("show_activity", {}), ("show_activity", {"developer": "Bernd", "command": "fire Axel"}),
                 ("proceed_one_day", {"days": True}), ("proceed_one_day", {"days": 2}),
                 ("proceed_one_day", {"days": "1"}),
                 ("review_two_developers", {"reviewer_1": "Bernd", "reviewer_2": "Stefanie", "document": "fabricated document"}),
                 ("review_two_developers", {"reviewer_1": "Bernd", "reviewer_2": "Bernd", "document": "specification"}),
                 ("correct_tested_code", {"developer": "Axel", "report_name": "invented-report"})]
        for command_id, arguments in cases:
            advice = self.advice()
            advice["recommendations"][0].update(command_id=command_id, arguments=arguments)
            with self.subTest(command=command_id, arguments=arguments):
                self.assert_error(self.store.submit_advice, advice, "invalid_arguments")

    def test_report_name_must_be_grounded_in_cited_result(self):
        self.store.ingest(self.event())
        result = self.event(2, "command.result")
        result["payload"]["kernel_output_raw"] = "Generated report module-test-17"
        self.store.ingest(result)
        advice = self.advice()
        advice["recommendations"][0].update(command_id="correct_tested_code", arguments={"developer": "Axel", "report_name": "module-test-17"}, evidence_event_ids=["e2"])
        _, record = self.store.submit_advice(advice)
        self.assertEqual(record["rendered_recommendations"][0]["command"], "Axel correct tested code using module-test-17")
        advice["advice_id"] = "a2"
        advice["recommendations"][0]["evidence_event_ids"] = ["e1"]
        self.assert_error(self.store.submit_advice, advice, "invalid_arguments")

    def test_server_renders_exact_review_wire_template(self):
        self.store.ingest(self.event())
        advice = self.advice()
        advice["recommendations"][0].update(command_id="review_three_developers", arguments={"reviewer_1": "Bernd", "reviewer_2": "Stefanie", "reviewer_3": "Axel", "document": "specification"})
        _, record = self.store.submit_advice(advice)
        self.assertEqual(record["rendered_recommendations"][0]["command"], "instruct Bernd and Stefanie and Axel review specification")

    def test_cross_stream_and_nonexistent_evidence_are_rejected(self):
        self.store.ingest(self.event())
        self.store.ingest(self.event(event_id="other-event", stream_id="other"))
        for evidence in (["does-not-exist"], ["other-event"]):
            advice = self.advice()
            advice["recommendations"][0]["evidence_event_ids"] = evidence
            self.assert_error(self.store.submit_advice, advice, "invalid_evidence")
            advice = self.advice(recommendations=[])
            advice["observations"][0]["evidence_event_ids"] = evidence
            self.assert_error(self.store.submit_advice, advice, "invalid_evidence")

    def test_new_events_stale_advice_and_old_revisions_are_rejected(self):
        self.store.ingest(self.event())
        advice = self.advice()
        self.store.submit_advice(advice)
        self.store.ingest(self.event(2))
        self.assertTrue(self.store.snapshot()["advice"][0]["stale"])
        self.assert_error(self.store.submit_advice, advice, "stale_advice", 409)

    def test_duplicate_events_do_not_stale_advice(self):
        self.store.ingest(self.event())
        self.store.submit_advice(self.advice())
        self.store.ingest(self.event())
        self.assertFalse(self.store.snapshot()["advice"][0]["stale"])

    def test_disconnect_and_pending_send_suppress_recommendations_until_result(self):
        self.store.ingest(self.event())
        self.store.ingest(self.event(2, "command.sent"))
        self.assertTrue(self.store.snapshot()["quality"]["uncertain"])
        self.assert_error(self.store.submit_advice, self.advice(), "context_uncertain", 409)
        self.store.ingest(self.event(3, "transport.uncertain"))
        self.assert_error(self.store.submit_advice, self.advice(), "context_uncertain", 409)
        self.store.submit_advice(self.advice(advice_id="observation-only", recommendations=[], missing_information=["Wait for an observed response"]))
        self.store.ingest(self.event(4, "command.result"))
        self.assertFalse(self.store.snapshot()["quality"]["uncertain"])
        self.store.submit_advice(self.advice())

    def test_callback_requires_terminal_result_and_has_no_answer_endpoint(self):
        self.store.ingest(self.event())
        self.store.ingest(self.event(2, "callback.requested"))
        self.assert_error(self.store.submit_advice, self.advice(), "context_uncertain", 409)
        self.store.ingest(self.event(3, "command.result"))
        self.assertFalse(self.store.snapshot()["quality"]["uncertain"])

    def test_different_command_cannot_resolve_a_pending_request(self):
        self.store.ingest(self.event())
        self.store.ingest(self.event(2, "command.sent"))
        self.store.ingest(self.event(3, "command.result", command="finish project"))
        self.assertTrue(self.store.snapshot()["quality"]["uncertain"])

    def test_late_events_use_sequence_for_state_and_ingestion_revision_for_advice(self):
        self.store.ingest(self.event())
        self.store.ingest(self.event(3, "command.result", turn_id=8))
        state = self.store.snapshot()
        self.assertEqual(state["quality"]["missing_sequences"], [2])
        self.assertEqual(state["revision"], 2)
        self.assert_error(self.store.submit_advice, self.advice(), "context_uncertain", 409)
        self.store.submit_advice(self.advice(recommendations=[]))
        self.store.ingest(self.event(2, "command.sent", turn_id=0))
        state = self.store.snapshot(context=True)
        self.assertEqual([event["sequence"] for event in state["events"]], [1, 2, 3])
        self.assertEqual(state["revision"], 3)
        self.assertEqual(state["turn_id"], 8)
        self.assertFalse(state["quality"]["uncertain"])
        self.assertTrue(state["advice"][0]["stale"])
        self.store.submit_advice(self.advice(advice_id="current"))

    def test_huge_sequence_gaps_have_bounded_output_and_block_advice(self):
        self.store.ingest(self.event(1_000_000))
        state = self.store.snapshot()
        self.assertEqual(len(state["quality"]["missing_sequences"]), 1000)
        self.assertTrue(state["quality"]["truncated"])
        self.assertTrue(state["quality"]["uncertain"])

    def test_advice_id_is_idempotent_and_cannot_be_repurposed(self):
        self.store.ingest(self.event())
        advice = self.advice()
        self.assertEqual(self.store.submit_advice(advice)[0], 201)
        self.assertEqual(self.store.submit_advice(advice)[0], 200)
        advice["summary"] = "Changed advice"
        self.assert_error(self.store.submit_advice, advice, "advice_conflict", 409)

    def test_state_and_deduplication_survive_restart(self):
        self.store.ingest(self.event())
        self.store.submit_advice(self.advice())
        self.store.close()
        self.store = AdviserStore(Path(self.directory.name))
        self.assertEqual(self.store.snapshot()["revision"], 1)
        self.assertEqual(len(self.store.snapshot()["advice"]), 1)
        self.assertTrue(self.store.ingest(self.event())[1]["duplicate"])

    def test_streams_are_scoped_and_default_to_latest_ingested(self):
        self.store.ingest(self.event())
        self.store.ingest(self.event(event_id="other-event", stream_id="other"))
        self.assertEqual(self.store.snapshot()["selected_stream_id"], "other")
        self.assertEqual([event["event_id"] for event in self.store.snapshot("stream")["events"]], ["e1"])
        self.assert_error(self.store.snapshot, "unknown", "stream_not_found", 404)


class HTTPTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.store = AdviserStore(Path(self.directory.name))
        # Exercise real HTTP parsing, headers, bodies and routing without needing
        # socket permissions in a restricted development environment.
        self.port = 18765
        self.server = SimpleNamespace(store=self.store, server_address=("127.0.0.1", self.port))

    def tearDown(self):
        self.store.close()
        self.directory.cleanup()

    def request(self, method, path, body=None, headers=None):
        body = body.encode("utf-8") if isinstance(body, str) else (body or b"")
        headers = {"Host": f"127.0.0.1:{self.port}", "Content-Length": str(len(body)), **(headers or {})}
        request = f"{method} {path} HTTP/1.1\r\n" + "".join(f"{key}: {value}\r\n" for key, value in headers.items()) + "\r\n"
        connection = MemoryConnection(request.encode("ascii") + body)
        AdviserHandler(connection, ("127.0.0.1", 12345), self.server)
        response = HTTPResponse(MemoryConnection(connection.sent))
        response.begin()
        data = response.read()
        status, response_headers = response.status, dict(response.getheaders())
        response.close()
        if response_headers.get("Content-Type", "").startswith("application/json"):
            data = json.loads(data)
        return status, data, response_headers

    def test_health_and_context_are_observer_only(self):
        status, data, _ = self.request("GET", "/api/v2/health")
        self.assertEqual(status, 200)
        self.assertEqual(data, {"status": "ok", "mode": "adviser", "simulator_control": False})
        self.assertEqual(self.request("GET", "/api/v2/context")[1]["revision"], 0)

    def test_no_execution_login_or_callback_answer_routes(self):
        for path in ("/execute", "/api/v2/execute", "/api/v2/commands", "/api/v2/login", "/api/v2/answer_callback"):
            with self.subTest(path=path):
                status, data, _ = self.request("POST", path, "{}", {"Content-Type": "application/json"})
                self.assertEqual(status, 404)
                self.assertEqual(data["error"]["code"], "not_found")

    def test_foreign_origins_and_hosts_are_rejected(self):
        for origin in ("https://example.com", "null", f"http://localhost:{self.port + 1}"):
            status, data, _ = self.request("GET", "/api/v2/state", headers={"Origin": origin})
            self.assertEqual((status, data["error"]["code"]), (403, "foreign_origin"))
        status, data, _ = self.request("GET", "/api/v2/state", headers={"Host": "attacker.example"})
        self.assertEqual((status, data["error"]["code"]), (400, "invalid_host"))
        status, _, _ = self.request("GET", "/api/v2/state", headers={"Origin": f"http://127.0.0.1:{self.port}"})
        self.assertEqual(status, 200)

    def test_invalid_json_extra_fields_and_large_bodies(self):
        for body in ('{"bad":', '{"kind":"x","kind":"y"}', '{"bad":NaN}', '[1,2]'):
            status, data, _ = self.request("POST", "/api/v2/events", body, {"Content-Type": "application/json"})
            self.assertEqual(status, 400)
            self.assertIn("error", data)
        status, data, _ = self.request("POST", "/api/v2/events", "{}", {"Content-Type": "application/json", "Content-Length": str(MAX_BODY_BYTES + 1)})
        self.assertEqual((status, data["error"]["code"]), (413, "body_too_large"))
        status, data, _ = self.request("POST", "/api/v2/events", "{}", {"Content-Type": "text/plain"})
        self.assertEqual((status, data["error"]["code"]), (400, "invalid_content_type"))

    def test_http_event_ingestion_and_duplicate_contract(self):
        body = json.dumps(self.store.examples()["events"][0])
        headers = {"Content-Type": "application/json"}
        self.assertEqual(self.request("POST", "/api/v2/events", body, headers)[0], 201)
        status, data, _ = self.request("POST", "/api/v2/events", body, headers)
        self.assertEqual(status, 200)
        self.assertTrue(data["duplicate"])
        self.assertEqual(self.request("GET", "/api/v2/state?stream_id=demo-specification-review")[1]["revision"], 1)

    def test_static_file_access_is_an_explicit_allowlist(self):
        for path in ("/login.txt", "/docs/../login.txt", "/docs/%2e%2e/login.txt", "/adviser/server.py", "/.runtime/adviser.sqlite3"):
            with self.subTest(path=path):
                self.assertEqual(self.request("GET", path)[0], 404)
        status, _, headers = self.request("GET", "/docs/README.md")
        self.assertEqual(status, 200)
        self.assertEqual(headers["X-Content-Type-Options"], "nosniff")
        self.assertIn("frame-ancestors 'none'", headers["Content-Security-Policy"])

    def test_query_parameters_are_validated(self):
        for path in ("/api/v2/state?stream_id=", "/api/v2/state?stream_id=a&stream_id=b", "/api/v2/state?password=x"):
            self.assertEqual(self.request("GET", path)[0], 400)

    def test_non_loopback_bind_is_rejected(self):
        with self.assertRaises(ValueError):
            AdviserHTTPServer(("0.0.0.0", 0), self.store)

    def test_live_tcp_smoke_when_socket_permissions_are_available(self):
        try:
            server = AdviserHTTPServer(("127.0.0.1", 0), self.store)
        except PermissionError:
            self.skipTest("Execution sandbox denies TCP socket creation; raw HTTP handler tests still run")
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            connection = HTTPConnection("127.0.0.1", server.server_address[1], timeout=3)
            connection.request("GET", "/api/v2/health")
            response = connection.getresponse()
            self.assertEqual(response.status, 200)
            self.assertFalse(json.loads(response.read())["simulator_control"])
            connection.close()
        finally:
            server.shutdown()
            server.server_close()
            thread.join()


class MemoryConnection:
    """Socket-shaped byte transport for BaseHTTPRequestHandler/HTTPResponse."""
    def __init__(self, data):
        self.received = BytesIO(data)
        self.sent = b""

    def makefile(self, *args):
        return self.received

    def sendall(self, data):
        self.sent += bytes(data)

    def settimeout(self, value):
        pass


class JSONTests(unittest.TestCase):
    def test_invalid_encoding_and_nonstandard_numbers_are_rejected(self):
        for body in (b"\xff", b"{\"x\":Infinity}", b"{\"x\":-Infinity}", b"{\"x\":0,\"x\":1}"):
            with self.subTest(body=body), self.assertRaises(ApiError):
                parse_json(body)


if __name__ == "__main__":
    unittest.main()

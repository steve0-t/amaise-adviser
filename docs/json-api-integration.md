# AMEISE adviser integration and web companion

This replaces the earlier agent-control proposal. The workflow is now **observe → explain → advise → human decision**. The existing AMEISE desktop client remains the application that communicates with the simulator and submits the user's actions.

A runnable local receiver and web dashboard are provided in [adviser/](../adviser/). They accept observation packets, display readable feedback, expose context to an external adviser, and validate its suggestions against a documented command policy. The Java observation hook and an actual model-backed adviser process are integration work described below; they are not connected automatically by starting the dashboard.

## 1. Data flow and responsibilities

```mermaid
flowchart LR
    Human[You] --> Client[AMEISE desktop client]
    Client <--> Simulator[AMEISE server]
    Client --> Observer[Planned observation hook]
    Observer -->|JSON observations| Receiver[Local companion service]
    Receiver --> Dashboard[Web dashboard]
    Receiver -->|Context and evidence| Agent[Adviser agent]
    Agent -->|Structured suggestions| Receiver
    Dashboard -->|Readable feedback and suggestions| Human
```

The observation hook copies information already sent or received by the desktop client. It does not create another simulator session. The receiver, browser, and adviser do not send simulator commands, answer simulator callbacks, log into AMEISE, or advance time. A suggested command can be copied and entered manually in the legacy client.

The web dashboard is a companion for understanding the game. It presents activity, feedback, pending questions, supporting evidence, and the command guide. It is not a replacement simulator transport or an implementation of the simulator's rules.

| Component | Responsibility |
| --- | --- |
| Existing desktop client | Login, selected game, simulator communication, command submission, polling, callbacks and its existing macro behavior. |
| Observation hook | Publish immutable, redacted copies of observed messages asynchronously. |
| Local receiver | Validate/store events, separate streams, expose context, validate adviser output and mark old advice stale. |
| Web dashboard | Translate packets into readable cards, inspect raw data, import recordings and show permitted suggestions. |
| Adviser process | Review evidence, identify uncertainty, propose appropriate allowed actions and explain why. |
| Human | Decide what to do and submit it through the desktop client. |

There is no `command.execute`, `command.answer`, `game.select`, or simulator-login endpoint in this API. The previous request schema and control examples have been replaced by observation and advice schemas. The new packet version is the string `"2"`.

## 2. Run the local companion

From the repository root, use the development shell, which supplies Python:

```sh
nix develop --command ./adviser/run.sh
```

Alternatively, with Python 3 available:

```sh
python3 adviser/server.py
```

Open **http://127.0.0.1:8765**. The service serves the dashboard and API from the same local origin. It binds to loopback; it is not intended to be published to Sites hosting or a public server. The interface starts without live data. **Load demo** imports clearly marked synthetic events and example advice. **Import packets** accepts a JSON object, JSON array, or JSONL recording.

The launcher's command-line options include `--port` and `--data-dir`. Runtime state is stored beneath `adviser/.runtime/` by default; keep recordings separate from source and credentials. See `python3 adviser/server.py --help` for the implemented options.

Starting the companion does not attach to `ClientV50.jar`, start an LLM, or generate real game observations. Until the observation hook is integrated, POST a sample event or import a recording to exercise the receiver and UI.

## 3. HTTP API

Use UTF-8 JSON with `Content-Type: application/json` for POST requests. JSONL is an import/recording format; the event endpoint accepts one JSON event per request. Invalid events receive a structured error and are not added to the stream.

| Endpoint | Producer/consumer | Behavior |
| --- | --- | --- |
| `GET /api/v2/health` | Launcher/browser | Reports service status and `simulator_control: false`. |
| `POST /api/v2/events` | Desktop observation hook/importer | Accepts one validated observation. New event: 201. Identical retry: 200. |
| `GET /api/v2/state?stream_id=...` | Browser | Returns selected stream, event history, advice, policy and observation quality. Without a stream ID, selects the most recently updated stream. |
| `GET /api/v2/context?stream_id=...` | Adviser | Returns the observed game/turn, current revision, evidence, policy and uncertainty for review. |
| `POST /api/v2/advice` | Adviser | Submits structured observations and suggestions tied to a specific stream revision. |
| `GET /api/v2/policy` | Browser/adviser | Returns the full command catalog and recommendation restrictions. |
| `GET /api/v2/examples` | Demo loader | Returns synthetic packets and example advice. It does not query AMEISE. |

The dashboard polls local state roughly every two seconds. An adviser may also poll context and review only when its revision changes. Neither kind of polling sends AMEISE requests.

Errors use `{ "error": { "code": string, "message": string } }`. Typical HTTP classifications are 400 for invalid data or forbidden recommendations, 404 for an unknown stream/route and 409 for conflicting event identity, stale context or incomplete evidence. Do not interpret a receiver rejection as failure of an action in the simulator: the receiver observes actions after they happen.

## 4. Observation packet contract

The [event schema](adviser-event.schema.json) is the machine-readable contract. [Example packets](adviser-events.jsonl) are synthetic recordings, not responses captured from a live server.

```json
{
  "api_version": "2",
  "event_id": "evt_demo_002",
  "stream_id": "stream_demo",
  "sequence": 2,
  "occurred_at": "2026-10-28T16:00:01Z",
  "source": "demo",
  "kind": "command.sent",
  "game": { "game_id": 17, "tournament_id": 4, "model_instance_id": 9 },
  "turn_id": 42,
  "payload": {
    "request_id": "observed_request_001",
    "command": "Bernd and Stefanie review specification"
  }
}
```

The ID/time values above are illustrative. `event_id` identifies an immutable event; keep it unchanged on delivery retries. `stream_id` identifies one observed game/branch session, not an authenticated server connection. Create a new stream on game selection or a user-driven restart/branch switch. Ordinary command progression stays in the same stream. Keep the original numeric game, tournament, model-instance and turn identifiers; do not increment turn IDs locally. Use `turn_id: null` when the client has not established one.

`sequence` starts at 1 within each stream and is allocated before events enter the publisher queue, so dropped packets leave a detectable gap. `occurred_at` is a UTC observation timestamp; the simulation's date is a separate raw string. `source` distinguishes `ameise_client`, `import`, and `demo` records. It is a provenance label, not proof that imported data came from a trusted client.

| Event kind | Required payload | Readable meaning |
| --- | --- | --- |
| `game.selected` | `name`, `simulation_date_raw` | A game was selected or an observation stream began. |
| `command.sent` | `request_id`, `command` | The desktop client sent a command; its result/effect is not yet established. |
| `command.result` | `request_id`, `command`, `feedback`, `kernel_output_raw`, `simulation_date_raw` | A terminal result and its raw feedback were observed. It may acknowledge a queued action rather than completed work. |
| `callback.requested` | `request_id`, `command`, `question`, `choices`, `previous_answer_ids` | The simulator asks for a selection; the human answers in the desktop client. |
| `transport.uncertain` | `request_id`, `command`, `message` | The client could not establish the result. Explain the uncertainty; do not replay the command. |

Callback choices contain numeric `id` and text `label` values. Preserve the real choice IDs and the full previous answer sequence. Array position is not a choice ID. The optional `alternatives_raw` string preserves unmodified upstream alternatives. If they cannot be parsed, publish `choices: []` with that raw text; the event still represents a pending question requiring human attention. A new callback for the same request replaces the displayed pending question; the eventual matching terminal result resolves it.

`request_id` correlates one emitted `CommandObject` and its poll/callback chain across send and result events. It is observation metadata created by the hook; the upstream DTOs do not supply a general JSON request identifier. Preserve it across that command's polling and callback exchanges. Every next emitted `CommandObject` gets a new ID, including each wire step of a user macro or multi-day Proceed. Keep `payload.command` as the original sent text for that command throughout; upstream result/callback strings are not proven byte-for-byte equal, and a mismatched command under the same request ID is treated as uncertain evidence.

The receiver accepts observations of any command, including excluded ones that the human used in the desktop client. Command restrictions filter **recommendations**, not history. Unknown or undocumented command text can still be observed; it cannot become a structured adviser recommendation without an explicit catalog entry.

Example ingestion from the repository root:

```sh
head -n 1 docs/adviser-events.jsonl | curl --fail-with-body \
  -H 'Content-Type: application/json' \
  --data-binary @- http://127.0.0.1:8765/api/v2/events
```

An acknowledgement reports `accepted`, `duplicate`, `stream_id` and the stream's receiver `revision`. It confirms storage in the companion, not an action's completion in AMEISE.

## 5. Ordering, duplicates, gaps and freshness

An identical `event_id` with the identical packet is idempotent. Reusing an ID for a different packet, or assigning different packets the same stream/sequence, is a conflict. The game identity is fixed within a stream. The receiver accepts out-of-order events and displays them by sequence.

Each unique ingestion increases the receiver's stream `revision`, including a late event with a lower sequence. This is why advice uses a revision instead of only the latest event ID. An adviser that reviewed revision 5 must not publish recommendations as current after revision 6 arrives.

State/context responses contain `quality.missing_sequences`, `quality.uncertain`, and `quality.truncated`. The current service returns all stored events; `truncated` means the missing-sequence detail list exceeded its 1000-entry display bound, not that stored history was omitted. Missing events, an unresolved sent command, a pending callback or an unresolved transport outcome must be treated as incomplete knowledge. When evidence is incomplete, the adviser can return observations and missing-information questions, but the receiver rejects executable command recommendations.

Old advice remains visible as history and is marked `stale`. The browser disables copying stale recommendations. Reading stale advice never refreshes it. Review the new context before issuing an updated suggestion.

The receiver's history contains only observations delivered to this companion. It is not a complete authoritative copy of all simulator state. A review's defect count, a developer's current task, budget totals or document quality must be shown as unknown unless an observed response supports the value. Do not calculate them from acknowledgement text or from how many commands were sent.

## 6. Adviser output contract

An external adviser reads `/api/v2/context`, then submits the [advice schema](adviser-advice.schema.json). [Example advice](adviser-demo-advice.json) demonstrates the shape. [Agent instructions](../adviser/AGENT_GUIDELINES.md) provide a reusable operating policy.

Advice contains `api_version`, `advice_id`, `stream_id`, `based_on_revision`, `policy_version`, `source`, `summary`, `observations`, `recommendations`, and `missing_information`. Each observation and recommendation cites `evidence_event_ids` from that stream. Recommendations use a catalog `command_id` and typed `arguments`, not a free-form command string. The receiver validates the policy and renders exact syntax itself.

For example, a recommendation entry can be:

```json
{
  "command_id": "show_specification",
  "arguments": {},
  "reason": "Inspect the specification before deciding whether to change the review plan.",
  "evidence_event_ids": ["evt_demo_003"]
}
```

Use the command IDs from the live policy response; this isolated object is not a complete POST body and does not establish that a command is currently available in a particular game.

The service verifies matching stream/revision/policy, known evidence IDs, recommendation eligibility, required arguments, value types and developer/document choices. It rejects extra arbitrary command fields, duplicate reviewers and multiline/control characters in text arguments. Text arguments are limited to 1–256 printable characters; a dynamic `report_name` must appear literally in the cited result's feedback or raw output. Numeric values are restricted to signed 64-bit integers. Only entries with `recommendation: "allowed"` can appear as suggested commands. Evidence references establish traceability; they do not automatically prove the model's reasoning or that a matching substring is a usable report name. The agent must still distinguish observations, assumptions and prerequisites.

The saved advice response includes `created_at`, computed `stale`, and `rendered_recommendations` with the exact command text. The browser may copy current permitted text for the human. There is no apply/execute endpoint behind that copy action.

Agent scheduling should use one active review per stream, a short quiet period after a burst of packets and a revision check just before submission. Repeated polling of unchanged context does not need another model call. A 409 for stale context means fetch and review the newer context; it does not authorize resubmitting an old recommendation with a new revision number.

## 7. Which commands may be recommended?

The [command policy](adviser-command-policy.json) is a full catalog grounded in [docs/README.md](README.md) and the [QA 200 command list](AMAISE_commandlist-qa200-en-v1.95.pdf). It contains all 42 PDF command forms plus one-day Proceed and retains commands even when the adviser cannot recommend them. The PDF distinguishes a clickable label from the actual syntax in parentheses; the latter is used in rendered suggestions.

| Classification | Adviser behavior | Default scope |
| --- | --- | --- |
| `allowed` | May recommend with relevant evidence and satisfied prerequisites; the human still executes manually. | Inspection/status, ordinary work, reviews, corrections, tests, staffing hires, customer meetings and one-day Proceed. |
| `manual_only` | Explain in the command guide; do not emit it as a proposed command. | `deliver system`, `fire <name>`, and notation changes. |
| `excluded` | Retain for understanding observed history; never recommend it. | `finish project`, `<name> quit activity`, `<name> finish activity`, `finish review document`. |

The README discourages `finish project`, `quit activity` and `finish activity`, particularly on an initial run. The PDF describes `finish review document` as aborting a review. Excluding these is the default adviser policy. Making delivery, firing and notation changes manual-only is an additional product choice to preserve deliberate human decisions; it is not a claim that the PDF forbids them.

Proceed is a client action described in the README and implemented in the existing client; it is not an entry in the PDF's three-page list. The initial recommendation policy limits it to `proceed 1`, preserving a chance to inspect intermediate feedback. The legacy client retains its existing automatic initial Proceed and multi-day behavior; the observer records what actually happens and does not alter it.

Commands being documented does not mean they are appropriate now. A recommendation must account for known hiring/activity state, document authorship, review membership, prerequisites, pending callbacks, missing feedback and current game phase. The README advises reviewers who did not author the artifact and warns that a second assignment can be delayed or lost when a developer is busy. If that context is absent, ask for an inspection or report missing information rather than asserting availability.

The catalog is the permitted recommendation vocabulary, not a reimplementation of the simulator's rule engine. If a runtime dictionary is later captured, it should further restrict the catalog, never silently expand it. Policy changes require a version change and revalidation of advice. All observations remain readable even if the policy changes later.

## 8. Web interface behavior

The local page provides:

- A stream selector, observation status and clear demo/import/live provenance.
- A readable activity timeline separating sent commands, returned feedback and unanswered questions.
- Adviser notes with evidence references, missing information and copyable permitted suggestions.
- A searchable command guide with allowed/manual-only/excluded filters, exact syntax and source/restriction explanations.
- A raw JSON inspector and packet import for troubleshooting and recorded sessions.
- A context export/copy option for an external agent.

There are no simulator controls in the browser. A pending callback directs the human to answer in AMEISE. Unknown metrics remain unknown. An agent that has not submitted advice is shown as absent/waiting; demo advice is labeled as example content. The normal viewer is deterministic and does not require an LLM to translate packet structure into readable cards.

Render imported feedback as text, preserving original strings in the inspector. The simulator output uses `@P`, `@W`, `@E` segments and control character 30 in some paths; display normalization may split these into readable lines but must not suppress warnings/errors. JSON strings and adviser prose are untrusted content for HTML rendering.

## 9. Add the observation hook to ClientV50

The receiver is implemented; these Java changes are integration guidelines for the next stage. The decompiled directory is not a clean rebuildable project. Preserve the original JAR's compiled protocol classes and serialization compatibility. Do not rebuild or replace every DTO just to add observation.

1. **Selection:** after a successful continue-game response has established game/model/turn state, start a stream and enqueue `game.selected`. Preserve the returned original game/tournament identity. Do not publish passwords, password hashes, usernames, email addresses or entire `PersonData` objects.
2. **Send:** in the existing sending path, after the message has been written/flushed, enqueue `command.sent` from its `CommandObject`. Allocate one observation `request_id` per emitted `CommandObject` before submission and preserve it while that command's interaction is pending. Callback-answer messages continue the same observed request. The next macro or Proceed step receives its own ID.
3. **Receive:** observe `CommandResultObject` and `CallbackResultObject` in the existing reply path. Publish a result with its returned turn/feedback, or a question with real choice IDs. The existing client continues to process those objects normally. Wait hints and polling remain inside the existing client; do not forward a wait hint as a terminal result.
4. **Failure:** when a write/read timeout or connection loss leaves the result uncertain, enqueue `transport.uncertain` if the process can still publish. A missing final event also leaves an unresolved sent command visible in the dashboard. Never retry a simulator action from the publisher.
5. **Publication:** use a bounded background queue to POST immutable observations to localhost. Queue/network failure must not block Swing's event thread, change return values, skip normal message handling or terminate the simulator client. Report queue drops locally; sequence gaps make incomplete evidence visible to the receiver.

The publisher may retry sending **the same observation packet** using its stable event ID. This is distinct from retrying the original simulator command. Keep the packet in a small local spool if reliable delivery across companion restarts is needed, and replay it in sequence when possible.

The existing protocol uses `MessageObject(2, RequestManagerObject(2, CommandObject))` for commands, type 3/result 2 for terminal results, type 3/result 1 for callbacks, and type 3/result 3 for wait hints. These are the observation points; JSON packets must never be written onto the simulator's object-stream socket.

For callback alternatives, the original parser has one-digit assumptions. Preserve the original alternatives when adapting it, and validate numeric ID parsing with real fixtures before publishing selectable choices. If choices cannot be interpreted, explain that limitation rather than inventing IDs or answering automatically. The currently accepted packet shape requires interpretable `choices`; an empty list can carry an unavailable selection while the human uses the original dialog.

## 10. Validation and completion boundaries

The implemented receiver/dashboard can be checked locally without a simulator account. Use the backend tests from the repository root:

```sh
python3 -m unittest adviser.test_server
```

Validate both JSON schemas, the synthetic events/advice, exact policy-template arguments, event deduplication/conflicts, cross-stream evidence, late events/revisions, stale advice, excluded/manual-only suggestions, pending questions, uncertain outcomes and the absence of execution routes. For the browser, verify empty/demo/import states, responsive layout, filters, raw inspection, copy behavior and stale/unknown labels.

Live integration requires observing a designated game in the original client, confirming that the hook does not change its behavior, and comparing captured send/result/callback sequences with the dashboard. An actual adviser additionally needs a separately configured model/tool runner that reads context and submits validated advice. Neither live connection nor model execution is implied by the demo.

## 11. Source map

| Concern | Reference |
| --- | --- |
| Rules and discouraged actions | [README.md](README.md), especially Using the simulator and Managing people, time, and cost |
| Exact labels, syntax and abort behavior | [Command list PDF](AMAISE_commandlist-qa200-en-v1.95.pdf), pages 1–3 |
| Client TCP send/flush | [SendingUnit.java](../ClientV50/sources/ameise/client/SendingUnit.java), `executeRequest` |
| Reply handling, wait hints and polling | [ReceivingUnit.java](../ClientV50/sources/ameise/client/ReceivingUnit.java), `receive`, `run`, `poll` |
| Game selection state and terminal objects | [LogicalUnit.java](../ClientV50/sources/ameise/client/LogicalUnit.java), `getObject`, `processDBObject`, `processResultObject` |
| Command/result/callback contents | [Communication package](../ClientV50/sources/ameise/shared/communication/) |
| GUI callback behavior, initial/multi-day Proceed | [SimulationPlus.java](../ClientV50/sources/ameise/client/components/SimulationPlus.java), `makeObject`, `processAnswerObject`, `processDistributedObject` |
| Callback IDs and previous answers | [CallbackList.java](../ClientV50/sources/ameise/client/components/CallbackList.java), `valueChanged`, `addElementToOldAnswers` |
| Output and alternatives parsing | [ParsingTool.java](../ClientV50/sources/ameise/client/util/ParsingTool.java), `filter`, `parseAlternatives` |
| Runtime dictionary and hidden commands | [SimulationGUIPlus.java](../ClientV50/sources/ameise/client/components/SimulationGUIPlus.java), `setDictionaryAndParameters` |
| Receiver implementation | [server.py](../adviser/server.py) |
| Agent operating instructions | [AGENT_GUIDELINES.md](../adviser/AGENT_GUIDELINES.md) |

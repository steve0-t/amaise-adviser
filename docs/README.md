# AMEISE QA 200: documentation summary

For the observer/adviser workflow and local web companion, see the [integration guide](json-api-integration.md), [event schema](adviser-event.schema.json), [example packets](adviser-events.jsonl), and [command recommendation policy](adviser-command-policy.json). The adviser suggests actions; you submit them in the AMEISE client.

This directory explains a simulated software project in which you act as the project manager. You choose developers, assign work, advance the simulation calendar, inspect feedback, and decide when the product is ready for the customer. The model follows a mostly waterfall sequence: requirements and specification, system and module design, coding, integration, testing, documentation, and delivery. Reviews and tests expose defects; you must assign corrections yourself. ([Introduction, pp. 1, 5–6](AMAISE_introduction-qa200-en-v1.95.pdf); [process overview](AMAISE_process_overview-v1.95.pdf))

## The assignment and success criteria

The customer, **Meyers & Co**, needs a new accounting system. A colleague has already collected an initial set of requirements. The estimate is **200 adjusted function points**, or roughly **9,800 lines of Ada 95 code**. All project documents and user manuals are to be in **German**. The brief is dated **7 October 2024** and sets a completion deadline of **4 July 2025**, with a budget of **€225,000**. The customer will perform an acceptance test. ([Job definition](Job_definition-qa200-en-v1.95.pdf))

Those dates come from the supplied job-definition PDF. A live game can use different dates: the recorded QS200-v19 run in [log.md](../log.md) starts in October 2026 and has a 2 July 2027 deadline. For live planning, use the current game's brief and subsequent resource reports rather than the PDF's example calendar.

| Customer requirement | Target |
| --- | ---: |
| Requirements represented in code | At least 95% |
| Code defects | At most 12 per 1,000 lines of code |
| Requirements represented in manuals | At least 95% |
| Manual defects | At most 0.5 per page |

These are delivery quality targets, not merely conditions for starting an activity. The simulator has separate minimum gates: work in a later phase normally needs its source document to be at least **50% complete**; integration needs at least **50% of requirements implemented in code**; and delivery needs at least **50% of requirements integrated**. Passing those gates alone would still fall short of the customer's 95% completeness targets. ([Introduction, pp. 6, 8–9](AMAISE_introduction-qa200-en-v1.95.pdf); [hints, pp. 4, 7](AMAISE_hints.pdf))

## How the project flows

1. **Start from the existing analysis and write the specification.** You can speak with the customer again; each meeting produces a new analysis document. Inspect the specification, arrange a review, and correct it when needed.
2. **Design the system and modules.** Each design document can be inspected, reviewed, and corrected. Later work depends on the quality and completeness of its source documents.
3. **Write and test code.** Implement the modules, inspect code quality, run module tests, and fix the defects reported. Code corrections may also need to be reflected in documents and integration.
4. **Integrate and test the whole system.** After enough code exists, integrate it, run integration and system tests, correct reported defects, and **integrate again after code changes** so the delivered build includes the latest versions.
5. **Write and review manuals.** Check that the German documentation covers the customer's requirements and meets its low error limit. A customer can join a documentation review.
6. **Request the customer's acceptance test, resolve findings, reintegrate if code changed, then deliver.** Complete any other important activity before `deliver system`. Customer feedback arrives after three simulation days of cleanup. ([Command list](AMAISE_commandlist-qa200-en-v1.95.pdf); [introduction, pp. 5–9](AMAISE_introduction-qa200-en-v1.95.pdf))

The [process overview](AMAISE_process_overview-v1.95.pdf) gives a concrete sequence *after coding*: module test → correction → integration → integration test → correction → integration → system test → correction → integration → acceptance test → correction → final integration. Its numbered diagram includes coding as step 1 and final integration as step 13. The diagram presents this as a possible successful sequence, not a rigid command schedule. In particular, it stresses that integration checks out the latest software versions. Review and correction loops can also occur earlier in the project.

## Managing people, time, and cost

The seven developers have different strengths. The hourly rates and broad strengths below come from the [qualification sheet](Developers-qa200-en-v1.95.pdf); the full sheet gives more detail about language, notation, and document experience.

| Developer | €/hour | Main strength | Relevant weakness |
| --- | ---: | --- | --- |
| Axel | 40 | Generalist; medium across listed skills | No particular high specialization |
| Bernd | 40 | Reviewing | Design and programming |
| Christine | 45 | Design | No particular low skill listed |
| Diana | 40 | Programming and Ada 95 | Application domain and specification |
| Richard | 50 | Specification and design | Reviewing |
| Stefanie | 40 | Reviewing | Programming |
| Thomas | 45 | Application domain | No particular low skill listed |

The project also has **€1,500 fixed cost** and charges **€15 per project manager hour**. Hired developers are paid even while idle, so hire in line with the work ahead. A released developer may become unavailable for up to **60 days**, making a later rehire uncertain. Release requires the person to be idle and out of reviews. Multiple developers can work on one document, but coordination consumes time. A review can overlap a developer's main task, though it slows that task. ([Developers, p. 1](Developers-qa200-en-v1.95.pdf); [introduction, pp. 2–3, 6–8](AMAISE_introduction-qa200-en-v1.95.pdf))

Use reviewers who **did not author the document**. The introduction warns that including an author can reduce the whole review team's effectiveness. The original author is, however, usually a good choice to *correct* the document afterward because they know it well. Developers normally handle one main task at a time; assigning a second task while they are busy may delay it or silently lose it. Verify a task has ended before reassigning that person. ([Introduction, pp. 7–8](AMAISE_introduction-qa200-en-v1.95.pdf); [hints, pp. 3, 9](AMAISE_hints.pdf))

## Using the simulator

The **Simulation Plus** tab provides clickable commands. An asterisk in the [command list](AMAISE_commandlist-qa200-en-v1.95.pdf) marks commands that return immediate feedback, such as `show me specification`, `show me code`, `information about <name>`, or `information about spent resources`. Other commands are queued when sent and normally take effect when you click **Proceed**. Acknowledgement that a command was sent only confirms its syntax; inspect the feedback after proceeding to see whether the assignment was accepted. Several commands can be issued for a day, but a sufficiently long list may itself advance the day. ([Introduction, pp. 1, 3–4](AMAISE_introduction-qa200-en-v1.95.pdf))

Useful command groups include:

| Purpose | Example commands |
| --- | --- |
| Staffing and status | `show me available developers`, `hire <name>`, `show me my team members`, `show me activity`, `information about spent resources` |
| Create work | `specify`, `design system`, `design modules`, `code`, `write documentation` |
| Assess quality | `show me specification`, `show me system design`, `show me module design`, `show me code`, `show me documentation`, review commands |
| Test and deliver | `test modules`, `integrate`, `test integration`, `test system`, `customer perform acceptance test`, `deliver system` |
| Repair | `correct specification`, `correct system design`, `correct module design`, `correct reviewed code`, `correct tested code`, `correct documentation`, `correct all documents` |

The exact parameterized wording is in the [command list](AMAISE_commandlist-qa200-en-v1.95.pdf). Use `correct all documents using <report name>` when a defect in an earlier artifact has propagated into downstream work. Reviews and tests only *report* problems; corrections are separate assignments. Review teams can have two or three developers, or two developers and the customer for the specification or manuals. ([Introduction, pp. 6–7, 9](AMAISE_introduction-qa200-en-v1.95.pdf))

Proceed advances to the next **working day**; weekends are skipped, while public holidays, vacation, and illness are not modeled. The date's hours and minutes have no project effect. Advancing several days at once saves clicks but postpones your chance to react to intermediate feedback. If an expected command seems to do nothing, check its prerequisites, whether the developer was hired or busy, and whether an earlier review has been corrected. Some status commands legitimately give no response when there is nothing to show. ([Introduction, pp. 4, 6–9](AMAISE_introduction-qa200-en-v1.95.pdf); [hints, pp. 5–7](AMAISE_hints.pdf))

`deliver system` ends the work and starts the final evaluation; the customer's response is shown after three simulation days of cleanup. The hints note that you may need to click **Proceed** again to see it. `finish project` instead aborts the project without the full delivery evaluation, so the introduction advises against using it. The introduction also advises avoiding `quit activity` and `finish activity` on a first run because they make document consistency harder to manage. ([Introduction, pp. 4–5, 9](AMAISE_introduction-qa200-en-v1.95.pdf); [hints, pp. 1, 7](AMAISE_hints.pdf))

## Documents in this directory

| File | What it contributes |
| --- | --- |
| [Job definition](Job_definition-qa200-en-v1.95.pdf) | Customer brief, dates, budget, implementation language, and measurable acceptance targets. |
| [Introduction](AMAISE_introduction-qa200-en-v1.95.pdf) | Simulator operation, lifecycle, dependencies, staffing, reviews, testing, and delivery behavior. |
| [Process overview](AMAISE_process_overview-v1.95.pdf) | Diagram of artifacts and a numbered test/correct/reintegrate sequence after coding. |
| [Command list](AMAISE_commandlist-qa200-en-v1.95.pdf) | Available actions and their command syntax. |
| [Developers' qualifications](Developers-qa200-en-v1.95.pdf) | Skills, hourly rates, and project overhead costs. |
| [Hints](AMAISE_hints.pdf) | Short reminders about staffing, reviews, time, prerequisites, and silent commands. |

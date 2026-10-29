# Text-first UI references for the AMEISE adviser

Research date: 28 October 2026. Scope: official product documentation and walkthroughs for reading project updates, investigating events, and reviewing advice. The observations below describe documented product behavior; the AMEISE adaptations are design recommendations, not features those products provide for this simulator.

Current design decision: retain the implemented overview and apply visual styling inspired by [Artcraft](https://getartcraft.com/). The layout proposals below remain research options, rather than instructions to restructure the current page.

## Recommended direction

Make the default page a readable project briefing, with a journal of meaningful interactions beneath it. Use a compact navigation rail, a wide reading column, and an optional context/advice panel. Keep complete packet history and raw JSON available through progressive disclosure.

The strongest combination is Linear's update-reading model, Basecamp's separation of catch-up summaries from chronology, incident.io's highlights and context, and Langfuse's grouped request/response inspection.

## References and transferable patterns

| Reference | Documented pattern | Proposed use in AMEISE |
| --- | --- | --- |
| [Linear Pulse](https://linear.app/docs/pulse) and [Inbox](https://linear.app/docs/inbox) | Pulse presents full written project updates and recency/custom feeds. Inbox separates priority updates and opens selected notifications in their issue context. | A compact update list with an explicit Needs attention filter. Read the selected interaction without losing the surrounding project context. |
| [Basecamp Account Activity](https://5.basecamp-help.com/article/1160-activity) | Separate Timeline and Wrap-up views; chronology grouped by date; project/person/keyword filters; links back to original context. | Separate Overview from All activity. Group updates by simulation date or artifact, and let readers jump from a summary to its supporting result. |
| [incident.io incident overview](https://incident.io/changelog/write-post-mortems-in-the-app#new-dashboard-ui) | Overview includes Highlights/All activity, relevant next actions, and condensed supporting information in a sidebar. | Lead with what changed, unresolved questions, and the next human decision. Place stream identity, freshness and evidence references beside the reading area. |
| [Langfuse trace inspection](https://langfuse.com/docs/observability/best-practices) | Related operations form a trace; meaningful input/output is emphasized, while full payloads belong in metadata. Noisy internal observations can be filtered out. | Group a sent command, callbacks and its result into one expandable interaction. Present readable feedback first and retain individual packets in an inspector. |
| [Sentry breadcrumbs](https://sentry.io/changelog/improved-breadcrumbs-on-issues/) | A short context view expands into a searchable/filterable drawer; the update explicitly removes an awkward inner scrolling area. | Show a short relevant history near the selected interaction. Open the complete packet details in a drawer instead of nesting scrolling panes inside every card. |
| [GitHub review comments panel](https://github.blog/changelog/2026-02-19-access-all-pull-request-comments-without-leaving-the-new-files-changed-page/) | Review discussion stays alongside the content being reviewed, with resolved/unresolved filters. | Keep adviser notes next to the feedback they discuss, with links to evidence. Distinguish unanswered questions from already addressed findings. |

The linked official pages include screenshots or product walkthrough material. This research does not claim hands-on testing of signed-in product accounts.

## 1. Organize the page around reading

The first screen should answer three questions in order:

1. What is the latest observed situation?
2. What changed, and what still needs attention?
3. What useful action could the human consider next?

Use a concise project title and a single metadata line for simulation date, provenance and last observation. Packet counts and receiver revision are debugging details; they do not need large summary cards. Large slogans, repeated introductory copy and decorative empty-state artwork should give way to actual feedback once data exists.

This is an AMEISE design recommendation. It follows the update/overview patterns above without importing their incident-management workflows or assuming simulator state that has not been observed.

## 2. Separate overview, interactions and packets

Use three reading depths:

| Depth | Contents | Default presentation |
| --- | --- | --- |
| Overview | Brief summary, unresolved questions, recent meaningful changes, adviser notes | Short paragraphs and a compact list |
| Interaction | One sent command, any follow-up questions, returned feedback and evidence | Expandable readable section or selected detail pane |
| Packet | Original JSON, IDs, receiver revision and transport metadata | Inspector drawer or explicit Raw data tab |

Grouping is presentation only. Preserve every original event and its source sequence. A macro can produce several wire interactions; combine them only when a real correlation supports the grouping. Do not join nearby unrelated events just because their timestamps are close.

The interaction should distinguish **Sent**, **Awaiting answer**, **Result received**, and **Outcome unknown**. Receiving a result does not prove that simulated work finished. For example, a review assignment acknowledgement belongs under Result received; actual review completion requires observed evidence.

## 3. Make adviser notes part of the context

For each note, show the recommendation in ordinary language, its reason, the evidence links and its freshness. Keep exact command syntax as a secondary copyable line. Use a short label such as Suggestion, Observed or Unconfirmed so the reader can distinguish facts from advice.

Clicking an evidence reference should open the relevant feedback while retaining the advice in view. A later observation marks the advice stale; keep that warning close to the affected recommendation and disable command copying as the current implementation does.

Pending callbacks belong near Needs attention. The human answers through the original client. Manual-only/excluded commands stay visible in the command guide with their existing policy explanations; a visual redesign must not change recommendation eligibility.

## 4. Suggested information architecture

| View | Main purpose |
| --- | --- |
| Overview | Catch up on the observed situation and identify the next decision. |
| Activity | Browse grouped interactions, with Highlights/All and date/developer/artifact filters where evidence supports them. |
| Command guide | Search documented forms and understand allowed/manual-only/excluded status. |
| Inspector | Examine individual packets and transport metadata. This can be a drawer rather than permanent navigation. |

Team and artifact summaries can be added only when the observation format captures reliable facts for them. Until then, do not create convincing-looking status tables from command text alone.

## 5. Proposed layout

This is a low-fidelity layout recommendation, not an implemented redesign:

```text
AMEISE       Meyers & Co                     Observed simulation date · source
             Overview   Activity   Command guide

Navigation   PROJECT BRIEFING                       ADVISER
             Latest observed situation              Suggested next step
             Short, factual paragraph               Reason and evidence

             NEEDS ATTENTION                        Command to copy
             Unanswered questions / unknowns        Freshness / limitations

             RECENT CHANGES                         CONTEXT
             Date                                   Selected stream
             · Meaningful interaction               Latest observation
               Readable returned feedback           Data quality
               Expand details →                     Source links

             Selected feedback / interaction        Raw packets → drawer
```

On smaller screens, use a single reading column with adviser notes immediately after the relevant briefing or interaction. Reserve color for status and selection; use typography, spacing and thin separators to establish the hierarchy. Start with roughly 15–16 px body text, comfortable line spacing and a bounded prose width, then verify with real long feedback and keyboard navigation. These measurements are proposed starting points, not conclusions from the product documentation.

## 6. What to change in the current companion

- Replace the large introductory heading and metric strip with a compact project header and briefing.
- Make one interaction the main unit of activity instead of one packet card.
- Introduce Highlights/All activity, preserving access to exhaustive evidence.
- Put adviser explanations next to the feedback they cite; keep raw data in a drawer.
- Use concise neutral labels, fewer framed boxes, and enough density to compare several updates without excessive scrolling.
- Preserve the current policy enforcement, demo labels, missing-data warnings and absence of simulator execution controls.

The first redesign pass should use the existing synthetic stream and an expanded fixture with long feedback, an unanswered callback, stale advice, missing packets and an unknown outcome. Judge it by whether a reader can find the latest observed situation, explain a recommendation's evidence, and distinguish acknowledgement from completed work.

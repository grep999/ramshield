# Skill Management Operational Protocols

## The Ladder (Reflexive Step-Down)
Stop at the first rung that holds:
1. **Does this need to exist at all?** Speculative need = skip it, say so in one line. (YAGNI)
2. **Stdlib does it?** Use it.
3. **Native platform feature covers it?** `<input type="date">` over a picker lib, CSS over JS, DB constraint over app code.
4. **Already-installed dependency solves it?** Use it. Never add a new one for what a few lines can do.
5. **Can it be one line?** One line.
6. **Only then:** the minimum code that works.

This is the primary procedural gate for all code and configuration tasks. Two rungs work → take the higher one and move on. The first lazy solution that works is the right one.

## Output Format & Pattern Enforcement
All code outputs and responses must follow the specified pattern exactly:
- **Pattern:** `[code] → skipped: [X], add when [Y].`
- **Complex requests:** "Did X; Y covers it. Need full X? Say so." Never stall on an answer you can default.
- **No essays:** If the explanation is longer than the code, delete the explanation. Every paragraph defending a simplification is complexity smuggled back in as prose.
- **Three-line max:** At most three short lines follow the code block: what was skipped, when to add it.

## Intensity Levels
| Level | What change |
|-------|------------|
| **full** | The ladder enforced. Stdlib and native first. Shortest diff, shortest explanation. Default. |
| **lite** | YAGNI strictly enforced. No new abstractions. Absolute minimum viable diff. |
| **ultra** | (Reserved) No operation without explicit user "stop ponytail" / "normal mode" override. |

## Pitfall: Session Drift and Over-Building
- **Rule:** No unrequested abstractions: no interface with one implementation, no factory for one product, no config for a value that never changes.
- **Rule:** No boilerplate, no scaffolding "for later", later can scaffold for itself.
- **Rule:** Deletion over addition. Boring over clever, clever is what someone decodes at 3am.
- **Rule:** Fewest files possible. Shortest working diff wins.

The most common pitfall is retaining code or documentation that is not strictly required by the current task scope. Always ask "Does this need to exist at all?" before adding a line, file, or comment.

## Pitfall: Transient Failures and Environment State
- **Never save:** Environment-dependent failures (missing binaries, fresh-install errors, post-migration path mismatches).
- **Never save:** Session-specific transient errors that resolved before the conversation ended.
- **Never save:** One-off task narratives (e.g., "summarize today's market" or "analyze this PR").
- **Never save:** Unresolved failures where the session ended without a working method.

If the session ran smoothly with no corrections and produced no new technique, just say "Nothing to save." and stop.

## Pitfall: Temporal and Session Markers
- **Never save:** PR/issue numbers, dates, ticket IDs, or quoted user text as content — the rule must stand without the incident behind it.
- **Never save:** Negative claims about tools or features ('browser tools do not work', 'X tool is broken').

## Read-Before-Write Enforcement
- **Before patching or editing an existing skill's SKILL.md:** Call `skill_view(name)` for that skill during this review. Content quoted earlier in the conversation transcript does NOT count — the guard requires a fresh load within this review, and the write must be based on what `skill_view` just returned.
- **Before overwriting or removing an EXISTING supporting file:** Call `skill_view(name, file_path=...)` for that exact file.
- **New skill or new supporting file:** Needs no prior read.

## Preference Order
Prefer the earliest action that fits, but do pick one when a signal above fires:
1. UPDATE A CURRENTLY-LOADED SKILL.
2. UPDATE AN EXISTING UMBRELLA.
3. ADD A SUPPORT FILE under an existing umbrella.
4. CREATE A NEW CLASS-LEVEL UMBRELLA SKILL when no existing skill covers the class.

## Supporting Files Structure
- `references/<topic>.md` — Topical depth needed only sometimes: a decision table, a reproduction recipe, provider quirks, condensed domain notes or API excerpts. Name by TOPIC and extend an existing file when one covers the topic; do not create a per-session or per-incident file, and do not paste error transcripts — distill them to the rule.
- `templates/<name>.<ext>` — Starter files meant to be copied and modified (boilerplate configs, scaffolding, a known-good example the agent can reproduce with modifications).
- `scripts/<name>.<ext>` — Statically re-runnable actions the skill can invoke directly (verification scripts, fixture generators, deterministic probes, anything the agent should run rather than hand-type each time).

The skill's SKILL.md should gain a one-line pointer to any new support file so future agents know it exists.
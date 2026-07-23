# Cassette lab, the experiment plan

> One job: define the cassette experiment: grow the recorder to capture and replay whole agent runs, in a separate lab repo, with a hard bar for merging anything back into core.

Adopted by the maintainers in July 2026 as part of refocusing EvalCore on the
offline, fail-closed CI gate. The experiment runs in a separate repository; core
only takes what proves itself. The deeper competitive rationale lives in the
private knowledge base and stays there.

## The bet

Cassettes (the record/replay cache) are the one thing EvalCore ships that no
competitor does: record a run once, replay it forever, offline, deterministic,
zero tokens. Scorers and judges are commodities; every framework has them, so we
do not grow there. Growth is in the cassette itself.

Today a cassette captures single request/response calls. The experiment: teach it
to capture an entire agent run, every model call, tool call, and environment
interaction, so the whole run replays offline exactly as it happened. No other
tool ships that.

The proving ground is agent tasksets: standardized exam bundles for agents (the
Harbor / verifiers-style format used by Terminal-Bench 2.0 and the Prime
Intellect Environments Hub), each holding a task, its environment, and its answer
key. Running one today requires Docker, Python, and network. If a cassette can
replay one offline and gate it in CI, the experiment has proven its point.

## Where it happens

- A separate repository in the `eval-core` org, proposed name `evalcore-lab`,
  seeded as a duplicate of this repo at the commit that creates it.
- Core (`eval-core/evalcore`) stays the stable foundation. Nothing lands here
  from the lab until the experiment passes the graduation bar below.
- The lab may bend process (spike-quality code, no TDD, throwaway branches) but
  not the identity: offline, zero telemetry, deterministic. If an approach needs
  to phone home, it is already dead.

## Phase 1: research before code

Deliverable: a findings doc in the lab repo answering these, with sources.

1. **What a cassette must capture.** For a full agent run: model calls, tool
   calls, environment reads and writes, ordering. What is the minimal recording
   that makes replay exact?
2. **Determinism under branching.** Agents take different paths run to run. What
   does byte-identical replay even mean for an agent, and where does it break?
3. **Taskset anatomy.** Is the format versioned and converging or still
   fragmenting? What does executing one require, and which subset is feasible
   fully offline?
4. **Model fit.** How do taskset tasks and their answer keys map onto EvalCore's
   case / scorer / gate model? What breaks?
5. **Prior art check.** Confirm nothing shipped since July 2026 already records
   and replays agent runs offline. If something did, the bet is re-evaluated
   before any code is written.

## Phase 2: spike

Only after Phase 1 answers hold up. Design the YAML surface first, per the
architecture rules: expect a new surface in `evals.yaml` that references a
taskset, not a new SDK. Then the thinnest end-to-end slice: run one real taskset,
record it into a cassette, replay it offline, gate on the exit code.

## Phase 3: measure

Run the same taskset through its native harness and through the lab build.
Compare setup burden, runtime, determinism across repeat runs, and offline
behavior. Write the numbers down whether they flatter us or not.

## Graduation bar (merge back into core)

All four, not three of four:

1. A real agent run records into a cassette and replays fully offline,
   deterministically.
2. The taskset spec is stable enough to version against.
3. The architecture rules in `CLAUDE.md` hold unmodified (protocols over SDKs,
   dependency direction, YAML-first, determinism, failures as data, exit codes).
4. A measured advantage over the native harness that a stranger could reproduce.

## Kill triggers

- Deterministic replay of agent runs proves impossible without faking it.
- Offline execution proves impossible without breaking zero-network.
- The taskset spec fragments with no dominant variant (then find another proving
  ground before killing the cassette work itself).
- The case/scorer/gate model needs distorting so badly that core would stop
  being core.

Killing the experiment is a valid result; the findings doc comes back to the
private knowledge base either way.

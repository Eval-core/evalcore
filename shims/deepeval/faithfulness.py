#!/usr/bin/env python3
# EvalCore subprocess scorer shim: DeepEval "faithfulness" (FaithfulnessMetric).
#
# HONESTY / COST WARNING
# ----------------------
# This metric calls an LLM ITSELF (via DeepEval). It is therefore:
#   * NOT covered by EvalCore's record/replay cache — every case is a live,
#     billable model call, and results are non-deterministic run to run.
#   * A poor fit for the PR / CI path. Put it in the scheduled / nightly tier.
# The deterministic, CACHED alternative on the PR path is EvalCore's native
# `judge` scorer given per-case `context`. Reach for this shim only when your
# team is standardized on DeepEval's exact metric definitions.
#
# Protocol (EvalCore subprocess scorer, v0):
#   stdin : {"input": str, "output": str, "expected": str|null,
#            "context": [str, ...]?}   # context omitted when the case has none
#   stdout: {"score": float, "passed": bool, "reason": str}
#
# Faithfulness measures whether the answer is grounded in the retrieval
# context. It requires per-case `context`; `expected` is not used.
#
# Provider keys: DeepEval reads its own provider credentials (e.g.
# OPENAI_API_KEY) from the environment. This script never reads, handles, or
# prints key values.
#
# Python 3.9+; stdlib-only at import time. DeepEval is lazy-imported inside
# score_with_deepeval() so `--check` and `python3 -m py_compile` work without
# it.
import json
import sys

METRIC = "faithfulness"
LIBRARY = "deepeval"


def read_case():
    """Parse the single JSON object on stdin, validating the protocol fields.

    Raises ValueError with a clear message on malformed input so callers can
    exit non-zero.
    """
    raw = sys.stdin.read()
    if not raw.strip():
        raise ValueError("no input on stdin: expected one JSON object")
    try:
        case = json.loads(raw)
    except json.JSONDecodeError as err:
        raise ValueError(f"stdin is not valid JSON: {err}") from err
    if not isinstance(case, dict):
        raise ValueError("stdin JSON must be an object")
    for field in ("input", "output"):
        if not isinstance(case.get(field), str):
            raise ValueError(f'missing required string field "{field}"')
    return case


def score_with_deepeval(case):
    """Map the EvalCore case to DeepEval's LLMTestCase and return (score, reason).

    API drift lives here: DeepEval's synchronous metric.measure() path.
    Field mapping: input=input, actual_output=output,
    retrieval_context=context. FaithfulnessMetric does not use `expected`.
    """
    from deepeval.metrics import FaithfulnessMetric
    from deepeval.test_case import LLMTestCase

    test_case = LLMTestCase(
        input=case["input"],
        actual_output=case["output"],
        retrieval_context=list(case["context"]),
    )
    metric = FaithfulnessMetric()
    metric.measure(test_case)
    reason = metric.reason or f"{LIBRARY} {METRIC} over {len(case['context'])} context chunk(s)"
    return float(metric.score), reason


def emit(score, passed, reason):
    json.dump({"score": score, "passed": passed, "reason": reason}, sys.stdout)
    sys.stdout.write("\n")


def main(argv):
    check = "--check" in argv[1:]

    try:
        case = read_case()
    except ValueError as err:
        print(f"{LIBRARY}/{METRIC}: {err}", file=sys.stderr)
        return 1

    if check:
        # Self-test: exercise the full protocol path (stdin -> validated fields
        # -> stdout) with a canned fake result, WITHOUT importing DeepEval or
        # calling an LLM. Metric preconditions (like requiring `context`) are
        # intentionally NOT enforced here — check mode validates the protocol,
        # not a metric's inputs — so CI can smoke-test offline.
        emit(1.0, True, "self-test")
        return 0

    if not isinstance(case.get("context"), list) or not case["context"]:
        print(
            f'{LIBRARY}/{METRIC}: this metric requires per-case context — '
            'add "context" to your dataset cases',
            file=sys.stderr,
        )
        return 1

    try:
        score, reason = score_with_deepeval(case)
    except ImportError as err:
        print(
            f"{LIBRARY}/{METRIC}: could not import DeepEval — "
            f"install with `pip install -r shims/deepeval/requirements.txt` ({err})",
            file=sys.stderr,
        )
        return 1
    except Exception as err:  # noqa: BLE001 - surface any scoring failure to EvalCore
        print(f"{LIBRARY}/{METRIC}: scoring failed: {err}", file=sys.stderr)
        return 1

    emit(score, score >= 0.5, reason)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

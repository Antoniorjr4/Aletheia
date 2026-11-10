#!/usr/bin/env python3
"""Read-only official AgentDojo suite discovery; not an attack or defense run."""
from __future__ import annotations

import argparse
import json
import pathlib

from adapter import BENCHMARK_VERSION, SUITES, manifest


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--suite", choices=sorted(SUITES), required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args()
    try:
        from agentdojo.task_suite.load_suites import get_suite
    except ImportError as exc:
        raise SystemExit("AgentDojo missing. Install official package: pip install agentdojo") from exc
    suite = get_suite(BENCHMARK_VERSION, args.suite)
    result = manifest(args.suite, suite)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n")
    print(json.dumps(result, indent=2, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

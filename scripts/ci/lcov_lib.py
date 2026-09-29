#!/usr/bin/env python3
"""Shared lcov helpers for the coverage scripts (#569 Phase 0)."""

from __future__ import annotations

import fnmatch
from collections import OrderedDict


def parse_scope(path: str) -> list[str]:
    patterns: list[str] = []
    with open(path, encoding="utf-8") as handle:
        for raw in handle:
            line = raw.strip()
            if not line or line.startswith("#"):
                continue
            patterns.append(line)
    return patterns


def in_scope(rel_path: str, patterns: list[str]) -> bool:
    return any(fnmatch.fnmatch(rel_path, pattern) for pattern in patterns)


def _relative(sf_path: str, root: str) -> str | None:
    normalized = sf_path.replace("\\", "/")
    prefix = root.replace("\\", "/").rstrip("/") + "/"
    if not normalized.startswith(prefix):
        return None
    rel = normalized[len(prefix) :]
    # Generated build-script / proc-macro artifacts live under target/ and would
    # otherwise inflate the workspace totals.
    if rel.startswith("target/"):
        return None
    return rel


def parse_lcov(path: str, root: str) -> "OrderedDict[str, dict]":
    """Parse an lcov tracefile into workspace-relative file records.

    Each record exposes the executable lines (`lines: {lineno: hits}`) plus the
    function counters. Files outside `root` are ignored so dependency sources do
    not skew workspace totals.
    """

    files: "OrderedDict[str, dict]" = OrderedDict()
    current: dict | None = None
    with open(path, encoding="utf-8", errors="replace") as handle:
        for raw in handle:
            line = raw.rstrip("\n")
            if line.startswith("SF:"):
                rel = _relative(line[3:], root)
                current = {
                    "rel": rel,
                    "lines": {},
                    "fnf": 0,
                    "fnh": 0,
                }
                if rel is not None:
                    files[rel] = current
            elif current is None:
                continue
            elif line.startswith("DA:"):
                parts = line[3:].split(",")
                if len(parts) >= 2:
                    try:
                        current["lines"][int(parts[0])] = int(parts[1])
                    except ValueError:
                        pass
            elif line.startswith("FNF:"):
                current["fnf"] = int(line[4:] or 0)
            elif line.startswith("FNH:"):
                current["fnh"] = int(line[5:] or 0)
    return files


def file_line_totals(record: dict) -> tuple[int, int]:
    lines = record["lines"]
    covered = sum(1 for hits in lines.values() if hits > 0)
    return len(lines), covered


def pct(covered: int, total: int) -> float:
    return (100.0 * covered / total) if total else 0.0

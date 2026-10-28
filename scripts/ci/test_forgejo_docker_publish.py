#!/usr/bin/env python3
"""Offline contract regression for `.forgejo/workflows/docker-publish.yml`.

The workflow publishes `latest` on `main` and the versioned tag only on `v*`
release tags. The pinned shared actions default `publish_latest` to true, so
the workflow must pass an explicit quoted `publish_latest: "false"` to both
publish steps. These checks read the real workflow, isolate each named step's
own `with:` block, and fail closed on in-memory variants that drop a trigger,
drop or misplace the flag, or add a pull-request trigger.

Standard library only: no PyYAML, no YAML 1.1 coercion, no node_modules.
Structure is checked with bounded indentation-aware text scanning for this
single fixed workflow.
"""

from __future__ import annotations

import unittest
from pathlib import Path

WORKFLOW_PATH = (
    Path(__file__).resolve().parents[2] / ".forgejo" / "workflows" / "docker-publish.yml"
)

RUST_ACTION = "https://forgejo.cloud1ful.com/actions/workflows/actions/build-rust-artifacts@v0.4.0"
TARGETS_ACTION = "https://forgejo.cloud1ful.com/actions/workflows/actions/publish-docker-targets@v0.4.0"
MANIFESTS_ACTION = "https://forgejo.cloud1ful.com/actions/workflows/actions/publish-docker-manifests@v0.4.0"
PINNED_ACTIONS = (RUST_ACTION, TARGETS_ACTION, MANIFESTS_ACTION)

PUBLISH_LATEST_LINE = 'publish_latest: "false"'


def _indent(line: str) -> int:
    return len(line) - len(line.lstrip(" "))


def _content_lines(lines: list[str]) -> list[str]:
    return [
        line
        for line in lines
        if line.strip() and not line.lstrip().startswith("#")
    ]


def _top_level_block(text: str, key: str) -> list[str]:
    lines = text.splitlines()
    for index, line in enumerate(lines):
        if line.rstrip() == f"{key}:":
            return _child_lines(lines[index + 1 :], _indent(line))
    return []


def _child_lines(lines: list[str], parent_indent: int) -> list[str]:
    block: list[str] = []
    for line in lines:
        if not line.strip() or line.lstrip().startswith("#"):
            if block:
                block.append(line)
            continue
        if _indent(line) <= parent_indent:
            break
        block.append(line)
    return block


def _child_block(lines: list[str], key: str, indent: int) -> list[str]:
    for index, line in enumerate(lines):
        if line.strip() == f"{key}:" and _indent(line) == indent:
            return _child_lines(lines[index + 1 :], indent)
    return []


def _unquote(value: str) -> str:
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
        return value[1:-1]
    return value


def _list_values(lines: list[str]) -> list[str]:
    values: list[str] = []
    for line in _content_lines(lines):
        stripped = line.strip()
        if stripped.startswith("- "):
            values.append(_unquote(stripped[2:]))
    return values


def _steps(text: str) -> list[list[str]]:
    steps: list[list[str]] = []
    current: list[str] | None = None
    for line in text.splitlines():
        if line.strip().startswith("- name:"):
            if current is not None:
                steps.append(current)
            current = [line]
        elif current is not None:
            current.append(line)
    if current is not None:
        steps.append(current)
    return steps


def _step_with_block(step: list[str]) -> list[str]:
    for index, line in enumerate(step):
        if line.strip() == "with:":
            return _child_lines(step[index + 1 :], _indent(line))
    return []


def _step_actions(step: list[str]) -> list[str]:
    actions: list[str] = []
    for line in _content_lines(step):
        stripped = line.strip()
        if stripped.startswith("uses: "):
            actions.append(stripped[len("uses: ") :].strip())
    return actions


def _step_blocks_by_action(text: str) -> dict[str, list[list[str]]]:
    blocks: dict[str, list[list[str]]] = {}
    for step in _steps(text):
        for action in _step_actions(step):
            blocks.setdefault(action, []).append(_step_with_block(step))
    return blocks


def _publish_latest_errors(with_lines: list[str], step_name: str) -> list[str]:
    errors: list[str] = []
    occurrences = 0
    for line in _content_lines(with_lines):
        stripped = line.strip()
        if not stripped.startswith("publish_latest:"):
            continue
        occurrences += 1
        if stripped != PUBLISH_LATEST_LINE:
            errors.append(
                f"{step_name}: publish_latest must be exactly {PUBLISH_LATEST_LINE} "
                f"(found {stripped!r})"
            )
    if occurrences != 1:
        errors.append(
            f"{step_name}: expected exactly one {PUBLISH_LATEST_LINE} in its own "
            f"with block, found {occurrences}"
        )
    return errors


def workflow_errors(text: str) -> list[str]:
    errors: list[str] = []

    trigger = _top_level_block(text, "on")
    if not trigger:
        errors.append("missing top-level on: trigger block")
    else:
        if any(
            line.strip() == "pull_request:" or line.strip().startswith("pull_request")
            for line in _content_lines(trigger)
        ):
            errors.append("pull_request trigger must not be defined")
        if not any(
            line.strip() == "workflow_dispatch:"
            for line in _content_lines(trigger)
        ):
            errors.append("workflow_dispatch trigger must be retained")
        push = _child_block(trigger, "push", 2)
        if not push:
            errors.append("push trigger block missing")
        else:
            branches = _list_values(_child_block(push, "branches", 4))
            tags = _list_values(_child_block(push, "tags", 4))
            if branches != ["main"]:
                errors.append(
                    f"push branches must be exactly ['main'], found {branches!r}"
                )
            if tags != ["v*"]:
                errors.append(f"push tags must be exactly ['v*'], found {tags!r}")

    for action in PINNED_ACTIONS:
        if text.count(action) != 1:
            errors.append(
                f"expected exactly one pinned action reference {action!r}, "
                f"found {text.count(action)}"
            )

    blocks = _step_blocks_by_action(text)
    for action, step_name in (
        (TARGETS_ACTION, "publish-docker-targets"),
        (MANIFESTS_ACTION, "publish-docker-manifests"),
    ):
        action_blocks = blocks.get(action, [])
        if len(action_blocks) != 1:
            errors.append(
                f"expected exactly one {step_name} step, found {len(action_blocks)}"
            )
            continue
        errors.extend(_publish_latest_errors(action_blocks[0], step_name))

    if "additional_tags:" in text:
        errors.append("workflow must not configure additional_tags")

    bun_lines = [
        line
        for line in _content_lines(text.splitlines())
        if "bun install" in line
    ]
    if len(bun_lines) != 1 or not all(
        "bun install" in line and "--minimum-release-age 604800" in line
        for line in bun_lines
    ):
        errors.append(
            "expected exactly one bun install line with --minimum-release-age 604800"
        )

    preserved = (
        "group: docker-publish-${{ github.ref }}",
        "platform: linux/amd64",
        "inline_cache_tag: latest-amd64",
        "builder_name_prefix: prompt-ferry",
    )
    for fragment in preserved:
        if text.count(fragment) != 1:
            errors.append(
                f"expected exactly one preserved workflow fact {fragment!r}, "
                f"found {text.count(fragment)}"
            )
    if text.count("${{ secrets.REGISTRY_TOKEN }}") != 2:
        errors.append(
            "expected both publish steps to use ${{ secrets.REGISTRY_TOKEN }}"
        )

    return errors


def assert_contract(test: unittest.TestCase, text: str) -> None:
    errors = workflow_errors(text)
    if errors:
        raise AssertionError(
            "invalid Forgejo docker publish workflow:\n- " + "\n- ".join(errors)
        )


def _read_workflow() -> str:
    return WORKFLOW_PATH.read_text(encoding="utf-8")


def _remove_once(text: str, fragment: str, last: bool = False) -> str:
    if last:
        index = text.rindex(fragment)
        return text[:index] + text[index + len(fragment) :]
    return text.replace(fragment, "", 1)


class ForgejoDockerPublishContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.workflow = _read_workflow()

    def test_checked_in_workflow_matches_contract(self) -> None:
        assert_contract(self, self.workflow)

    def test_main_branch_removed_fails(self) -> None:
        variant = _remove_once(self.workflow, '      - "main"\n')
        self.assertNotEqual(variant, self.workflow)
        self.assertTrue(workflow_errors(variant))

    def test_release_tag_filter_removed_fails(self) -> None:
        variant = _remove_once(self.workflow, '      - "v*"\n')
        self.assertNotEqual(variant, self.workflow)
        self.assertTrue(workflow_errors(variant))

    def test_pull_request_trigger_added_fails(self) -> None:
        variant = self.workflow.replace(
            "  workflow_dispatch:\n", "  pull_request:\n  workflow_dispatch:\n", 1
        )
        self.assertNotEqual(variant, self.workflow)
        self.assertTrue(workflow_errors(variant))

    def test_targets_publish_latest_removed_fails(self) -> None:
        variant = _remove_once(self.workflow, f"          {PUBLISH_LATEST_LINE}\n")
        self.assertNotEqual(variant, self.workflow)
        errors = workflow_errors(variant)
        self.assertTrue(any("publish-docker-targets" in error for error in errors), errors)

    def test_manifests_publish_latest_removed_fails(self) -> None:
        variant = _remove_once(
            self.workflow, f"          {PUBLISH_LATEST_LINE}\n", last=True
        )
        self.assertNotEqual(variant, self.workflow)
        errors = workflow_errors(variant)
        self.assertTrue(any("publish-docker-manifests" in error for error in errors), errors)

    def test_publish_latest_only_on_manifests_fails(self) -> None:
        variant = _remove_once(self.workflow, f"          {PUBLISH_LATEST_LINE}\n")
        variant = variant.rstrip("\n") + f"\n          {PUBLISH_LATEST_LINE}\n"
        errors = workflow_errors(variant)
        self.assertTrue(any("publish-docker-targets" in error for error in errors), errors)
        self.assertTrue(any("publish-docker-manifests" in error for error in errors), errors)

    def test_unquoted_publish_latest_fails(self) -> None:
        variant = self.workflow.replace(PUBLISH_LATEST_LINE, "publish_latest: false")
        self.assertNotEqual(variant, self.workflow)
        self.assertTrue(workflow_errors(variant))

    def test_manifest_action_removed_fails(self) -> None:
        variant = _remove_once(
            self.workflow,
            f"        uses: {MANIFESTS_ACTION}\n",
        )
        self.assertNotEqual(variant, self.workflow)
        self.assertTrue(workflow_errors(variant))

    def test_dependency_age_policy_removed_fails(self) -> None:
        variant = self.workflow.replace(" --minimum-release-age 604800", "", 1)
        self.assertNotEqual(variant, self.workflow)
        self.assertTrue(workflow_errors(variant))


if __name__ == "__main__":
    unittest.main()

#!/usr/bin/env python3
"""
Bootstrap a GitHub project for risk-hotpath-hft and seed it with issues.

Behavior
- Create the project and capture its number or id
- Create each issue with gh issue create in its repo
- Add each issue to the project and set fields: Track, Priority, Size, Stage, and Sprint to Sprint 1
- At the end, print the project URL and the URLs of the created issues
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import textwrap
from dataclasses import dataclass
from typing import Dict, List, Optional


@dataclass
class FieldConfig:
    name: str
    data_type: str
    options: Optional[List[str]] = None


ISSUES = [
    {
        "title": "Rust workspace and simulator scaffold",
        "body": textwrap.dedent(
            """
            Title: Rust workspace and simulator scaffold
              Body:
              - Create Cargo workspace, risk_core crate, simulator that emits ticks at a fixed rate and logs counters
              - Acceptance: cargo test passes, make run_sim prints increasing counters
              Labels: track hot, stage build, priority P1, size M
            """
        ).strip(),
        "track": "hot",
        "priority": "P1",
        "size": "M",
        "stage": "build",
    },
    {
        "title": "Order book skeleton",
        "body": textwrap.dedent(
            """
            Title: Order book skeleton
              Body:
              - Implement a minimal price time order book with add, cancel, and top of book snapshot
              - Acceptance: unit tests cover add and cancel and top of book, cargo test passes
              Labels: track hot, stage build, priority P1, size M
            """
        ).strip(),
        "track": "hot",
        "priority": "P1",
        "size": "M",
        "stage": "build",
    },
    {
        "title": "Price collar and credit checks with tests",
        "body": textwrap.dedent(
            """
            Title: Price collar and credit checks with tests
              Body:
              - Implement price_collar(limit_px, ref_px, pct) and a simple credit limit check
              - Acceptance: unit tests pass including boundary cases
              Labels: track hot, stage build, priority P1, size S
            """
        ).strip(),
        "track": "hot",
        "priority": "P1",
        "size": "S",
        "stage": "build",
    },
    {
        "title": "ClickHouse tables and writer stub",
        "body": textwrap.dedent(
            """
            Title: ClickHouse tables and writer stub
              Body:
              - Create tables ticks and risk_outcomes and add a writer stub behind a feature flag
              - Acceptance: ClickHouse up in compose, writer creates tables and inserts one sample row
              Labels: track hot, stage build, priority P2, size S
            """
        ).strip(),
        "track": "hot",
        "priority": "P2",
        "size": "S",
        "stage": "build",
    },
]

REQUIRED_FIELDS: List[FieldConfig] = [
    FieldConfig(name="Track", data_type="SINGLE_SELECT", options=["hot"]),
    FieldConfig(name="Priority", data_type="SINGLE_SELECT", options=["P1", "P2"]),
    FieldConfig(name="Size", data_type="SINGLE_SELECT", options=["S", "M"]),
    FieldConfig(name="Stage", data_type="SINGLE_SELECT", options=["build"]),
    FieldConfig(name="Sprint", data_type="TEXT"),
]


class CliError(RuntimeError):
    """Raised when a GitHub CLI command fails."""


def run(cmd: List[str], **kwargs) -> str:
    try:
        return subprocess.check_output(cmd, text=True, **kwargs).strip()
    except subprocess.CalledProcessError as exc:  # pragma: no cover - defensive
        raise CliError(f"Command failed: {' '.join(cmd)}\n{exc.output}") from exc


def require_cli() -> None:
    if shutil.which("gh") is None:
        raise CliError("GitHub CLI (gh) is required on PATH")
    if os.environ.get("GITHUB_TOKEN") is None:
        raise CliError("GITHUB_TOKEN must be set for GitHub CLI authentication")


def infer_owner_repo() -> tuple[str, str]:
    remote_url = run(["git", "config", "--get", "remote.origin.url"])
    if remote_url.endswith(".git"):
        remote_url = remote_url[:-4]
    ssh_match = re.match(r"git@github.com:(?P<owner>[^/]+)/(?P<repo>.+)", remote_url)
    https_match = re.match(r"https?://github.com/(?P<owner>[^/]+)/(?P<repo>.+)", remote_url)
    match = ssh_match or https_match
    if not match:
        raise CliError(f"Unable to parse owner/repo from remote URL: {remote_url}")
    return match.group("owner"), match.group("repo")


def create_project(owner: str, title: str) -> dict:
    output = run(
        [
            "gh",
            "project",
            "create",
            "--owner",
            owner,
            "--title",
            title,
            "--format",
            "json",
        ]
    )
    data = json.loads(output)
    return {"id": data["id"], "number": data["number"], "url": data["url"]}


def fetch_fields(owner: str, project_number: int) -> Dict[str, dict]:
    output = run(
        [
            "gh",
            "project",
            "field-list",
            str(project_number),
            "--owner",
            owner,
            "--format",
            "json",
        ]
    )
    fields = json.loads(output)
    field_lookup: Dict[str, dict] = {}
    for field in fields:
        options = {opt["name"]: opt for opt in field.get("options", [])} if field.get("options") else {}
        field_lookup[field["name"]] = {
            "id": field["id"],
            "dataType": field["dataType"],
            "options": options,
        }
    return field_lookup


def ensure_fields(owner: str, project_number: int) -> Dict[str, dict]:
    field_lookup = fetch_fields(owner, project_number)
    for config in REQUIRED_FIELDS:
        if config.name in field_lookup:
            continue
        cmd = [
            "gh",
            "project",
            "field-create",
            str(project_number),
            "--owner",
            owner,
            "--name",
            config.name,
            "--data-type",
            config.data_type,
            "--format",
            "json",
        ]
        if config.options:
            cmd.extend(["--single-select-options", ",".join(config.options)])
        run(cmd)
    return fetch_fields(owner, project_number)


def create_issue(owner: str, repo: str, issue: dict) -> str:
    output = run(
        [
            "gh",
            "issue",
            "create",
            "--repo",
            f"{owner}/{repo}",
            "--title",
            issue["title"],
            "--body",
            issue["body"],
        ]
    )
    return output.splitlines()[-1].strip()


def add_item(owner: str, project_number: int, issue_url: str) -> dict:
    output = run(
        [
            "gh",
            "project",
            "item-add",
            str(project_number),
            "--owner",
            owner,
            "--url",
            issue_url,
            "--format",
            "json",
        ]
    )
    return json.loads(output)


def set_single_select(project_id: str, item_id: str, field: dict, value: str) -> None:
    option = field["options"].get(value)
    if not option:
        raise CliError(f"Value '{value}' missing from field {field}")
    run(
        [
            "gh",
            "project",
            "item-edit",
            "--id",
            item_id,
            "--project-id",
            project_id,
            "--field-id",
            field["id"],
            "--single-select-option-id",
            option["id"],
        ]
    )


def set_text(project_id: str, item_id: str, field: dict, value: str) -> None:
    run(
        [
            "gh",
            "project",
            "item-edit",
            "--id",
            item_id,
            "--project-id",
            project_id,
            "--field-id",
            field["id"],
            "--text",
            value,
        ]
    )


def apply_fields(project: dict, item: dict, fields: Dict[str, dict], issue: dict, sprint: str) -> None:
    set_single_select(project["id"], item["id"], fields["Track"], issue["track"])
    set_single_select(project["id"], item["id"], fields["Priority"], issue["priority"])
    set_single_select(project["id"], item["id"], fields["Size"], issue["size"])
    set_single_select(project["id"], item["id"], fields["Stage"], issue["stage"])
    set_text(project["id"], item["id"], fields["Sprint"], sprint)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Create a GitHub project and seed issues")
    parser.add_argument("--owner", help="GitHub org or user. Defaults to git remote owner", default=None)
    parser.add_argument("--repo", help="Repository name. Defaults to git remote repo", default=None)
    parser.add_argument("--project-title", help="Project title", default="risk_hotpath_hft")
    parser.add_argument("--sprint", help="Sprint label to assign", default="Sprint 1")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    require_cli()
    owner, repo = infer_owner_repo()
    if args.owner:
        owner = args.owner
    if args.repo:
        repo = args.repo

    project = create_project(owner, args.project_title)
    fields = ensure_fields(owner, project["number"])

    print(f"Created project {project['url']} (number={project['number']})")
    print("Creating issues and adding to project...\n")

    issue_urls: List[str] = []
    for issue in ISSUES:
        issue_url = create_issue(owner, repo, issue)
        item = add_item(owner, project["number"], issue_url)
        apply_fields(project, item, fields, issue, args.sprint)
        issue_urls.append(issue_url)
        print(f"- {issue['title']}: {issue_url}")

    print("\nAll items created. Project URL:")
    print(project["url"])
    print("Issues:")
    for url in issue_urls:
        print(url)


if __name__ == "__main__":
    try:
        main()
    except CliError as exc:  # pragma: no cover - defensive
        sys.stderr.write(f"Error: {exc}\n")
        sys.exit(1)

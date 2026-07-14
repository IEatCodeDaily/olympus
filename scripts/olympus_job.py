#!/usr/bin/env python3
"""Submit trusted build/test jobs to the Olympus FxBuilder JobRunner."""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

DEFAULT_NODE = "fxbuilder"
DEFAULT_URL = "http://127.0.0.1:8799"
DEFAULT_ORIGIN = "https://olympus.entelechia.cloud"
DEFAULT_TOKEN_FILE = Path.home() / ".olympus" / "token"
WORKSPACE_ROOT = "/srv/olympus/jobs/workspaces"
WORKSPACE_PATTERN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")
ENV_ALLOWLIST = [
    "PATH",
    "HOME",
    "CARGO_HOME",
    "CARGO_TARGET_DIR",
    "RUSTUP_HOME",
    "BUN_INSTALL",
    "CI",
    "TERM",
]
RSYNC_EXCLUDES = [
    ".git/",
    ".worktrees/",
    "target/",
    "node_modules/",
    "dist/",
    "build/",
    "coverage/",
    ".cache/",
]


def validate_workspace(value: str) -> str:
    if value in {"", ".", ".."} or not WORKSPACE_PATTERN.fullmatch(value):
        raise ValueError(
            "workspace must be 1-64 characters: letters, digits, dot, underscore, or dash"
        )
    return value


def validate_cwd(value: str | None) -> str | None:
    if value is None:
        return None
    path = Path(value)
    if path.is_absolute() or ".." in path.parts:
        raise ValueError("cwd must remain inside the FxBuilder job root")
    return value


def build_payload(
    *,
    node: str,
    argv: list[str],
    cwd: str | None,
    timeout: int,
    max_output: int,
) -> dict[str, Any]:
    if not argv or not argv[0]:
        raise ValueError("command must contain a program")
    if timeout < 1:
        raise ValueError("timeout must be positive")
    if max_output < 1:
        raise ValueError("max output must be positive")
    return {
        "nodeId": node,
        "argv": argv,
        "envAllowlist": ENV_ALLOWLIST,
        "cwd": validate_cwd(cwd),
        "timeoutSecs": timeout,
        "maxOutputBytes": max_output,
    }


def build_rsync_command(source: Path, workspace: str) -> list[str]:
    validate_workspace(workspace)
    source = source.expanduser().resolve()
    if not source.is_dir():
        raise ValueError(f"source directory does not exist: {source}")
    command = ["rsync", "-a", "--delete", "--delete-excluded"]
    command.extend(f"--exclude={pattern}" for pattern in RSYNC_EXCLUDES)
    command.extend(
        [
            f"{source}/",
            f"fxbuilder:{WORKSPACE_ROOT}/{workspace}/",
        ]
    )
    return command


def result_exit_code(result: dict[str, Any]) -> int:
    code = result.get("exitCode")
    if isinstance(code, int):
        return max(0, min(code, 255))
    if result.get("timedOut"):
        return 124
    if result.get("cancelled"):
        return 130
    return 1


class OlympusClient:
    def __init__(self, *, base_url: str, origin: str, token_file: Path) -> None:
        self.base_url = base_url.rstrip("/")
        self.origin = origin
        try:
            self.token = token_file.expanduser().read_text().strip()
        except OSError as error:
            raise RuntimeError(f"cannot read Olympus token file {token_file}: {error}") from error
        if not self.token:
            raise RuntimeError(f"Olympus token file is empty: {token_file}")

    def request(
        self,
        method: str,
        path: str,
        payload: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        data = None if payload is None else json.dumps(payload).encode()
        request = urllib.request.Request(
            self.base_url + path,
            data=data,
            method=method,
            headers={
                "Authorization": f"Bearer {self.token}",
                "Origin": self.origin,
                "Content-Type": "application/json",
            },
        )
        try:
            with urllib.request.urlopen(request, timeout=15) as response:
                body = response.read()
        except urllib.error.HTTPError as error:
            detail = error.read().decode(errors="replace")
            raise RuntimeError(f"Olympus {method} {path} returned {error.code}: {detail}") from error
        except OSError as error:
            raise RuntimeError(f"Olympus {method} {path} failed: {error}") from error
        if not body:
            return {}
        try:
            return json.loads(body)
        except json.JSONDecodeError as error:
            raise RuntimeError(f"Olympus returned invalid JSON for {path}") from error

    def dispatch(self, payload: dict[str, Any]) -> str:
        response = self.request("POST", "/api/jobs", payload)
        job_id = response.get("jobId")
        if not isinstance(job_id, str) or not job_id:
            raise RuntimeError(f"Olympus dispatch omitted jobId: {response}")
        return job_id

    def get_job(self, job_id: str) -> dict[str, Any]:
        return self.request("GET", f"/api/jobs/{job_id}")

    def cancel(self, job_id: str) -> None:
        self.request("DELETE", f"/api/jobs/{job_id}")


def strip_remainder_separator(argv: list[str]) -> list[str]:
    return argv[1:] if argv[:1] == ["--"] else argv


def sync_workspace(source: Path, workspace: str) -> None:
    command = build_rsync_command(source, workspace)
    print(f"syncing {source.expanduser().resolve()} -> fxbuilder:{WORKSPACE_ROOT}/{workspace}", file=sys.stderr)
    subprocess.run(command, check=True)


def run_job(
    client: OlympusClient,
    *,
    node: str,
    argv: list[str],
    cwd: str | None,
    timeout: int,
    max_output: int,
    poll_interval: float,
) -> int:
    payload = build_payload(
        node=node,
        argv=argv,
        cwd=cwd,
        timeout=timeout,
        max_output=max_output,
    )
    job_id = client.dispatch(payload)
    print(f"submitted {job_id} to {node}", file=sys.stderr)
    try:
        while True:
            record = client.get_job(job_id)
            if record.get("status") == "completed":
                output = record.get("output", "")
                if output:
                    print(output, end="" if output.endswith("\n") else "\n")
                if record.get("truncated"):
                    print("warning: job output was truncated", file=sys.stderr)
                if record.get("timedOut"):
                    print("job timed out", file=sys.stderr)
                if record.get("cancelled"):
                    print("job was cancelled", file=sys.stderr)
                return result_exit_code(record)
            time.sleep(poll_interval)
    except KeyboardInterrupt:
        print(f"cancelling {job_id}", file=sys.stderr)
        try:
            client.cancel(job_id)
        except RuntimeError as error:
            print(f"cancellation failed: {error}", file=sys.stderr)
        return 130


def add_run_arguments(parser: argparse.ArgumentParser, *, default_cwd: str | None = None) -> None:
    parser.add_argument("--node", default=DEFAULT_NODE)
    parser.add_argument("--cwd", default=default_cwd)
    parser.add_argument("--timeout", type=int, default=3600)
    parser.add_argument("--max-output", type=int, default=16 * 1024 * 1024)
    parser.add_argument("command", nargs=argparse.REMAINDER)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", default=os.environ.get("OLYMPUS_URL", DEFAULT_URL))
    parser.add_argument("--origin", default=os.environ.get("OLYMPUS_ORIGIN", DEFAULT_ORIGIN))
    parser.add_argument(
        "--token-file",
        type=Path,
        default=Path(os.environ.get("OLYMPUS_TOKEN_FILE", DEFAULT_TOKEN_FILE)),
    )
    parser.add_argument("--poll-interval", type=float, default=0.5)
    subparsers = parser.add_subparsers(dest="action", required=True)

    run_parser = subparsers.add_parser("run", help="dispatch a command through Olympus")
    add_run_arguments(run_parser)

    sync_parser = subparsers.add_parser("sync", help="synchronize a source tree to FxBuilder")
    sync_parser.add_argument("--source", type=Path, required=True)
    sync_parser.add_argument("--workspace", required=True)

    sync_run_parser = subparsers.add_parser(
        "sync-run", help="synchronize a source tree, then dispatch a command"
    )
    sync_run_parser.add_argument("--source", type=Path, required=True)
    sync_run_parser.add_argument("--workspace", required=True)
    add_run_arguments(sync_run_parser)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        if args.action in {"sync", "sync-run"}:
            workspace = validate_workspace(args.workspace)
            sync_workspace(args.source, workspace)
            if args.action == "sync":
                return 0
            args.cwd = f"workspaces/{workspace}"

        command = strip_remainder_separator(args.command)
        client = OlympusClient(
            base_url=args.url,
            origin=args.origin,
            token_file=args.token_file,
        )
        return run_job(
            client,
            node=args.node,
            argv=command,
            cwd=args.cwd,
            timeout=args.timeout,
            max_output=args.max_output,
            poll_interval=args.poll_interval,
        )
    except (RuntimeError, ValueError, subprocess.CalledProcessError) as error:
        print(f"olympus-job: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())

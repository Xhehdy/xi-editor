#!/usr/bin/env python3
"""Benchmark Glyph IPC edit/content round-trips over the framed JSON socket."""

from __future__ import annotations

import argparse
import json
import os
import socket
import statistics
import struct
import subprocess
import time
from pathlib import Path
from typing import Any


def recv_exact(sock: socket.socket, n: int) -> bytes:
    data = bytearray()
    while len(data) < n:
        chunk = sock.recv(n - len(data))
        if not chunk:
            raise RuntimeError("socket closed")
        data.extend(chunk)
    return bytes(data)


def send_message(sock: socket.socket, message: dict[str, Any]) -> dict[str, Any]:
    payload = json.dumps(message, separators=(",", ":")).encode("utf-8")
    sock.sendall(struct.pack(">I", len(payload)))
    sock.sendall(payload)

    resp_len = struct.unpack(">I", recv_exact(sock, 4))[0]
    resp = recv_exact(sock, resp_len)
    return json.loads(resp)


def percentile(values: list[float], p: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    k = (len(ordered) - 1) * p
    f = int(k)
    c = min(f + 1, len(ordered) - 1)
    if f == c:
        return ordered[f]
    return ordered[f] * (c - k) + ordered[c] * (k - f)


def summarize(times_us: list[float]) -> dict[str, float]:
    mean_us = statistics.fmean(times_us)
    return {
        "mean_us": mean_us,
        "p50_us": percentile(times_us, 0.50),
        "p95_us": percentile(times_us, 0.95),
        "throughput_req_s": (1_000_000.0 / mean_us) if mean_us > 0 else 0.0,
    }


def wait_for_socket(path: Path, timeout_s: float = 10.0) -> None:
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        if path.exists():
            return
        time.sleep(0.02)
    raise TimeoutError(f"socket did not appear: {path}")


def connect_with_retry(path: Path, timeout_s: float = 10.0) -> socket.socket:
    deadline = time.time() + timeout_s
    last_err: OSError | None = None

    while time.time() < deadline:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            sock.connect(str(path))
            return sock
        except OSError as err:
            last_err = err
            sock.close()
            time.sleep(0.02)

    raise TimeoutError(f"could not connect to socket {path}: {last_err}")


def benchmark(socket_path: Path, loops_edit: int, loops_get_64k: int) -> dict[str, Any]:
    with connect_with_retry(socket_path) as sock:

        hello = send_message(
            sock,
            {
                "Hello": {
                    "client_version": "ipc-bench-1.0.0",
                    "capabilities": {"patch_streaming": True},
                }
            },
        )
        if "Welcome" not in hello:
            raise RuntimeError(f"unexpected hello response: {hello}")

        new_view = send_message(sock, {"NewView": {}})
        if "ViewCreated" not in new_view:
            raise RuntimeError(f"unexpected new_view response: {new_view}")
        view_id = int(new_view["ViewCreated"]["view_id"])
        revision = int(new_view["ViewCreated"]["revision"])

        # Seed content with one byte so benchmark can do delete+insert each loop.
        seed = send_message(
            sock,
            {
                "ApplyEdit": {
                    "view_id": view_id,
                    "start": 0,
                    "deleted_len": 0,
                    "inserted_text": "a",
                    "base_revision": revision,
                }
            },
        )
        if "ApplyPatch" not in seed:
            raise RuntimeError(f"unexpected seed response: {seed}")
        revision = int(seed["ApplyPatch"]["revision"])

        edit_times_us: list[float] = []
        for i in range(loops_edit):
            inserted = "a" if i % 2 == 0 else "b"
            start_ns = time.perf_counter_ns()
            response = send_message(
                sock,
                {
                    "ApplyEdit": {
                        "view_id": view_id,
                        "start": 0,
                        "deleted_len": 1,
                        "inserted_text": inserted,
                        "base_revision": revision,
                    }
                },
            )
            end_ns = time.perf_counter_ns()
            if "ApplyPatch" not in response:
                raise RuntimeError(f"unexpected apply_edit response: {response}")
            revision = int(response["ApplyPatch"]["revision"])
            edit_times_us.append((end_ns - start_ns) / 1000.0)

        # Build a 64KB document and measure GetContent RTT.
        large_view = send_message(sock, {"NewView": {}})
        if "ViewCreated" not in large_view:
            raise RuntimeError(f"unexpected large new_view response: {large_view}")
        large_view_id = int(large_view["ViewCreated"]["view_id"])
        large_revision = int(large_view["ViewCreated"]["revision"])

        seed_large = send_message(
            sock,
            {
                "ApplyEdit": {
                    "view_id": large_view_id,
                    "start": 0,
                    "deleted_len": 0,
                    "inserted_text": "x" * (64 * 1024),
                    "base_revision": large_revision,
                }
            },
        )
        if "ApplyPatch" not in seed_large:
            raise RuntimeError(f"unexpected large seed response: {seed_large}")

        get_times_us: list[float] = []
        for _ in range(loops_get_64k):
            start_ns = time.perf_counter_ns()
            response = send_message(sock, {"GetContent": {"view_id": large_view_id}})
            end_ns = time.perf_counter_ns()
            if "SetContent" not in response:
                raise RuntimeError(f"unexpected get_content response: {response}")
            get_times_us.append((end_ns - start_ns) / 1000.0)

        return {
            "apply_edit_1b": {"loops": loops_edit, **summarize(edit_times_us)},
            "get_content_64kb": {"loops": loops_get_64k, **summarize(get_times_us)},
        }


def main() -> int:
    parser = argparse.ArgumentParser(description="Benchmark Glyph IPC path")
    parser.add_argument(
        "--core-binary",
        default="target/release/glyph",
        help="Path to glyph core binary (used when --spawn-core is set)",
    )
    parser.add_argument(
        "--socket-path",
        default="/tmp/glyph.sock",
        help="Unix socket path",
    )
    parser.add_argument(
        "--loops-edit",
        type=int,
        default=1200,
        help="ApplyEdit benchmark loop count",
    )
    parser.add_argument(
        "--loops-get-64k",
        type=int,
        default=180,
        help="GetContent(64KB) benchmark loop count",
    )
    parser.add_argument(
        "--output",
        default="",
        help="Optional path to write JSON output",
    )
    parser.add_argument(
        "--spawn-core",
        action="store_true",
        help="Spawn core process for benchmark",
    )
    args = parser.parse_args()

    socket_path = Path(args.socket_path)

    core_proc: subprocess.Popen[Any] | None = None
    try:
        if args.spawn_core:
            core_proc = subprocess.Popen(
                [args.core_binary],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            wait_for_socket(socket_path)
        elif not socket_path.exists():
            raise FileNotFoundError(
                f"socket not found at {socket_path}; run with --spawn-core or start core first"
            )

        results = benchmark(socket_path, args.loops_edit, args.loops_get_64k)
        results["timestamp_unix_s"] = time.time()
        results["socket_path"] = str(socket_path)

        output = json.dumps(results, indent=2)
        print(output)

        if args.output:
            Path(args.output).write_text(output + os.linesep, encoding="utf-8")
        return 0
    finally:
        if core_proc is not None:
            core_proc.terminate()
            try:
                core_proc.wait(timeout=2)
            except subprocess.TimeoutExpired:
                core_proc.kill()
                core_proc.wait(timeout=2)


if __name__ == "__main__":
    raise SystemExit(main())

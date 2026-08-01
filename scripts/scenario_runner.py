#!/usr/bin/env python3
"""
RamShield Scenario Benchmark Runner

Load 99 scenario definitions from scenarios_99.json, run them in waves (chaotic + predictable),
collect metrics from the running RamShield, and generate a comparative benchmark report.

Requirements:
- Running RamShield instance at 127.0.0.1:7890 (IPC) and 127.0.0.1:9999 (dashboard)
- Ensure attack scripts have proper inbound to the IPC port (report_connections)

Usage:
  python3 scripts/scenario_runner.py run --target 127.0.0.1:7890 --snapshot-url http://127.0.0.1:9999/api/snapshot
  python3 scripts/scenario_runner.py run --file scripts/scenarios_99.json --mode wave --chaos
  python3 scripts/scenario_runner.py export-report --out benchmark_report.md

Output:
- per-scenario metrics: events_sent, throughput, blocks, threat_score, latency, ram_usage
- wave aggregations and comparative stats (mean, stddev, percentiles)
- markdown report for sharing
"""

from __future__ import annotations
import argparse
import json
import time
import threading
import subprocess
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor, as_completed
from typing import Any, Dict, List, Optional, Tuple
import sys

# Shared stats collector
class ScenarioStats:
    def __init__(self):
        self.events_sent = 0
        self.batches_sent = 0
        self.errors = 0
        self.reconnects = 0
        self.throughput = 0.0
        self.blocks = 0
        self.threat_score = 0.0
        self.ram_bytes = 0
        self.ram_limit_mb = 0
        self.cpu_usage = 0.0
        self.uptime_secs = 0
        self.start_ts = time.perf_counter()

    def update_from_snapshot(self, snap: Dict[str, Any]) -> None:
        # Dashboard snapshot fields (mirroring engine.rs DashboardSnapshot)
        self.blocks = snap.get("blocked_total", 0)
        self.throughput = snap.get("events_ingested", 0)
        self.threat_score = snap.get("ram_pct", 0.0)  # proxy for threat pressure
        self.ram_bytes = snap.get("ram_bytes", 0)
        self.ram_limit_mb = snap.get("ram_limit_mb", 0)
        self.cpu_usage = snap.get("cpu_usage", 0.0)
        self.uptime_secs = snap.get("uptime_secs", 0)

    def summary(self) -> Dict[str, Any]:
        return {
            "events_sent": self.events_sent,
            "batches_sent": self.batches_sent,
            #!/usr/bin/env python3
            """
            RamShield Scenario Benchmark Runner

            Load 99 scenario definitions from scenarios_99.json, run them in waves (chaotic + predictable),
            collect metrics from the running RamShield, and generate a comparative benchmark report.

            Requirements:
            - Running RamShield instance at 127.0.0.1:7890 (IPC) and 127.0.0.1:9999 (dashboard)
            - Ensure attack scripts have proper inbound to the IPC port (report_connections)

            Usage:
              python3 scripts/scenario_runner.py run --target 127.0.0.1:7890 --snapshot-url http://127.0.0.1:9999/api/snapshot
              python3 scripts/scenario_runner.py run --file scripts/scenarios_99.json --mode wave --chaos
              python3 scripts/scenario_runner.py export-report --out benchmark_report.md

            Output:
            - per-scenario metrics: events_sent, throughput, blocks, threat_score, latency, ram_usage
            - wave aggregations and comparative stats (mean, stddev, percentiles)
            - markdown report for sharing
            """

            from __future__ import annotations
            import argparse
            import json
            import time
            import subprocess
            from pathlib import Path
            from typing import Any, Dict, List, Optional, Tuple
            import sys
            import statistics

            # Load scenarios from JSON
            def load_scenarios(path: str) -> List[Dict[str, Any]]:
                with open(path, "r", encoding="utf-8") as f:
                    data = json.load(f)
                scenarios = data.get("scenarios", {})
                # Convert to list of scenario dicts, preserving the ID
                scenario_list = []
                for sid, spec in scenarios.items():
                    scenario_list.append({"id": sid, **spec})
                # Sort by category then ID for deterministic ordering
                scenario_list.sort(key=lambda x: (x["category"], x["id"]))
                return scenario_list

            # Determine if we should run sequentially, waves, or chaotic
            def build_waves(scenarios: List[Dict[str, Any]], mode: str) -> List[List[Dict[str, Any]]]:
                if mode == "sequential":
                    return [scenarios]
                elif mode == "wave":
                    # Group by category, each category a wave
                    waves_dict: Dict[str, List[Dict[str, Any]]] = {}
                    for sc in scenarios:
                        waves_dict.setdefault(sc["category"], []).append(sc)
                    return list(waves_dict.values())
                elif mode == "chaotic":
                    # Shuffle categories each wave, no deterministic order
                    import random
                    random.shuffle(scenarios)
                    # Split into 10 waves for demonstration
                    wave_size = max(1, len(scenarios) // 10)
                    waves = []
                    for i in range(0, len(scenarios), wave_size):
                        waves.append(scenarios[i:i+wave_size])
                    return waves
                else:
                    # default sequential
                    return [scenarios]

            # Collect dashboard snapshot
            def fetch_snapshot(snapshot_url: str) -> Optional[Dict[str, Any]]:
                try:
                    import urllib.request
                    with urllib.request.urlopen(snapshot_url, timeout=5) as resp:
                        return json.load(resp)
                except Exception:
                    return None

            def main():
                parser = argparse.ArgumentParser(description="Run RamShield scenario benchmark suite")
                subparsers = parser.add_subparsers(dest="command", required=True)

                # Run command
                run_parser = subparsers.add_parser("run", help="Run scenarios against a live RamShield instance")
                run_parser.add_argument("--target-host", default="127.0.0.1", help="RamShield IPC host")
                run_parser.add_argument("--target-port", type=int, default=7890, help="RamShield IPC port")
                run_parser.add_argument("--snapshot-url", default="http://127.0.0.1:9999/api/snapshot", help="Dashboard snapshot endpoint")
                run_parser.add_argument("--scenarios", default="scripts/scenarios_99.json", help="Path to scenarios JSON")
                run_parser.add_argument("--workers", type=int, default=128, help="Workers for attack scripts")
                run_parser.add_argument("--mode", choices=["sequential", "wave", "chaotic"], default="sequential", help="Execution mode")
                run_parser.add_argument("--dry-run", action="store_true", help="Print commands without executing")
                run_parser.add_argument("--sleep-before", type=float, default=5.0, help="Sleep before each scenario (seconds)")
                run_parser.add_argument("--sleep-after", type=float, default=2.0, help="Sleep after each scenario (seconds)")

                # Export report command
                export_parser = subparsers.add_parser("export-report", help="Generate a markdown report from last run (not implemented)")
                export_parser.add_argument("--out", default="benchmark_report.md", help="Output markdown path")

                args = parser.parse_args()

                if args.command == "run":
                    scenarios = load_scenarios(args.scenarios)
                    waves = build_waves(scenarios, args.mode)

                    print(f"Loaded {len(scenarios)} scenarios")
                    print(f"Running in {len(waves)} wave(s) (mode={args.mode})")
        
                    all_results = []

                    for wave_idx, wave in enumerate(waves):
                        print(f"\n=== Wave {wave_idx + 1} ===")
                        wave_results = []
                        for sc in wave:
                            print(f"Scenario {sc['id']}: {sc.get('description', '')[:80]}...")
                            # Determine scenario-specific parameters
                            profile = sc.get("profile", sc.get("mode", "l7_http_flood"))
                            target_ip = sc.get("target_ip", "10.255.0.99")
                            duration = sc.get("duration_sec", 30)
                            workers = sc.get("workers", args.workers)
                
                            if args.dry_run:
                                print(f"  (dry-run) would run profile {profile} with target {target_ip} for {duration}s")
                                wave_results.append({
                                    "id": sc["id"],
                                    "category": sc["category"],
                                    "profile": profile,
                                    "dry_run": True,
                                    "timestamp": time.time(),
                                })
                                continue
                    
                            # Run the scenario attack
                            cmd = [
                                sys.executable,
                                str(Path(__file__).parent / "attack_nexus.py"),
                                "run",
                                "--host", args.target_host,
                                "--port", str(args.target_port),
                                "--workers", str(workers),
                                "--target", target_ip,
                                "--profile", profile,
                                "--duration", str(duration),
                            ]
                            print(f"  Executing: {' '.join(cmd)}")
                            try:
                                result = subprocess.run(cmd, capture_output=True, text=True, cwd=Path(__file__).parent)
                                print(f"  Attack completed (exit code {result.returncode})")
                                if result.stdout:
                                    print(f"  Output: {result.stdout[:200]}...")
                                # Wait for RamShield to process and collect snapshot
                                time.sleep(args.sleep_after)
                                snap = fetch_snapshot(args.snapshot_url)
                                if snap:
                                    print(f"    blocked={snap.get('blocked_total')}, ingested={snap.get('events_ingested')}, throughput={snap.get('events_ingested')}")
                                    wave_results.append({
                                        "id": sc["id"],
                                        "category": sc["category"],
                                        "profile": profile,
                                        "target_ip": target_ip,
                                        "duration": duration,
                                        "attack_exit_code": result.returncode,
                                        "attack_stdout": result.stdout[:500] if result.stdout else None,
                                        "attack_stderr": result.stderr[:500] if result.stderr else None,
                                        "snapshot": snap,
                                        "timestamp": time.time(),
                                    })
                                else:
                                    print("    snapshot fetch failed")
                                    wave_results.append({
                                        "id": sc["id"],
                                        "category": sc["category"],
                                        "profile": profile,
                                        "target_ip": target_ip,
                                        "duration": duration,
                                        "attack_exit_code": result.returncode,
                                        "attack_stdout": result.stdout[:500] if result.stdout else None,
                                        "attack_stderr": result.stderr[:500] if result.stderr else None,
                                        "snapshot": None,
                                        "timestamp": time.time(),
                                    })
                            except Exception as e:
                                print(f"  Error running scenario: {e}")
                                wave_results.append({
                                    "id": sc["id"],
                                    "category": sc["category"],
                                    "profile": profile,
                                    "target_ip": target_ip,
                                    "duration": duration,
                                    "error": str(e),
                                    "timestamp": time.time(),
                                })
                
                            # Sleep between scenarios
                            time.sleep(args.sleep_before)
                
                        all_results.extend(wave_results)
        
                    # Save results to JSON
                    results_path = Path(__file__).parent / "benchmark_results.json"
                    with open(results_path, "w", encoding="utf-8") as f:
                        json.dump({
                            "scenarios_run": len(all_results),
                            "waves": len(waves),
                            "timestamp": time.time(),
                            "results": all_results,
                        }, f, indent=2)
        
                    print(f"\n=== Benchmark complete ===")
                    print(f"Results saved to: {results_path}")
        
                    # Print summary
                    successful = [r for r in all_results if "snapshot" in r and r["snapshot"] is not None]
                    print(f"Successful runs: {len(successful)}/{len(all_results)}")
                    if successful:
                        total_blocks = sum(r["snapshot"].get("blocked_total", 0) for r in successful)
                        total_ingested = sum(r["snapshot"].get("events_ingested", 0) for r in successful)
                        print(f"Total blocks detected: {total_blocks}")
                        print(f"Total events processed: {total_ingested}")

                elif args.command == "export-report":
                    print(f"Export report to {args.out}")
                    # Placeholder: read last run results from benchmark_results.json and format
                    results_path = Path(__file__).parent / "benchmark_results.json"
                    if results_path.exists():
                        with open(results_path, "r", encoding="utf-8") as f:
                            data = json.load(f)
            
                        # Generate markdown report
                        with open(args.out, "w", encoding="utf-8") as f:
                            f.write("# RamShield Scenario Benchmark Report\n\n")
                            f.write(f"Generated: {time.ctime()}\n\n")
                            f.write(f"Total scenarios run: {data.get('scenarios_run', 0)}\n")
                            f.write(f"Waves: {data.get('waves', 0)}\n\n")
                
                            for result in data.get("results", []):
                                f.write(f"## Scenario {result['id']}\n\n")
                                f.write(f"- Category: {result['category']}\n")
                                f.write(f"- Profile: {result.get('profile', 'N/A')}\n")
                                f.write(f"- Target IP: {result.get('target_ip', 'N/A')}\n")
                                f.write(f"- Duration: {result.get('duration', 'N/A')}s\n")
                                f.write(f"- Timestamp: {time.ctime(result['timestamp'])}\n\n")
                    
                                if "snapshot" in result and result["snapshot"]:
                                    snap = result["snapshot"]
                                    f.write("### Metrics\n")
                                    f.write(f"- Blocks detected: {snap.get('blocked_total', 0)}\n")
                                    f.write(f"- Events ingested: {snap.get('events_ingested', 0)}\n")
                                    f.write(f"- Events rejected: {snap.get('events_rejected', 0)}\n")
                                    f.write(f"- CPU usage: {snap.get('cpu_usage', 0.0)}%\n")
                                    f.write(f"- RAM usage: {snap.get('ram_bytes', 0)} bytes / {snap.get('ram_limit_mb', 0)} MB\n")
                                else:
                                    f.write("### Status: Snapshot unavailable\n\n")
                    
                                f.write("---\n\n")
            
                        print(f"Report written to: {args.out}")
                    else:
                        print("No benchmark results found. Run with 'run' first.")

            if __name__ == "__main__":
                main()
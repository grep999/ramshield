#!/usr/bin/env python3
"""RamShield Test Suite Integration Runner

Load-tests the four core integration scripts (suite.py, metrics_smoke.py, dashboard_integration.py, final_integration.py)
from the scripts/ directory and aggregates results into a single, actionable report.

Usage:
  python3 scripts/integration_suite.py
  python3 scripts/integration_suite.py --t19-only
  python3 scripts/integration_suite.py --cold-start-only
  python3 scripts/integration_suite.py --full

The script expects all integration tests to be present in the scripts/ directory.
"""

import argparse
import pathlib
import subprocess
import sys
import time

SCRIPT_DIR = pathlib.Path(__file__).resolve().parent

CORE_INTEGRATION_SCRIPTS = [
    "suite.py",
    "metrics_smoke.py", 
    "dashboard_integration.py",
    "final_integration.py"
]


def verify_scripts_exist() -> list[pathlib.Path]:
    """Check if all integration scripts are present."""
    missing = []
    for script in CORE_INTEGRATION_SCRIPTS:
        if not (SCRIPT_DIR / script).exists():
            missing.append(script)
    
    if missing:
        print(f"❌ MISSING INTEGRATION SCRIPTS: {', '.join(missing)}")
        print(f"📁 Available scripts in {SCRIPT_DIR}:")
        for f in SCRIPT_DIR.iterdir():
            if f.is_file() and f.suffix == ".py":
                print(f"  - {f.name}")
        print(f"\n🚫 Cannot run integration tests without these core scripts.")
        print(f"💡 Create them with basic functionality or revert to a working state.")
        sys.exit(1)
    
    print(f"✅ All {len(CORE_INTEGRATION_SCRIPTS)} core integration scripts found")
    return [SCRIPT_DIR / script for script in CORE_INTEGRATION_SCRIPTS]


def run_integration_tests(scripts: list[pathlib.Path], profile: str = "all") -> dict:
    """Run integration test suite with given profile."""
    print(f"\n🔄 Running integration tests with profile: {profile}")
    print("=" * 60)
    
    results = {
        "status": "success",
        "profile": profile,
        "scripts_run": [],
        "failures": [],
        "warnings": [],
        "execution_time": 0,
        "timestamp": time.time()
    }
    
    start_time = time.time()
    
    try:
        # Run suite.py first (main orchestrator)
        suite_script = SCRIPT_DIR / "suite.py"
        if suite_script.exists():
            print(f"\n1️⃣  Executing {suite_script.name} (main orchestrator)...")
            # These are integration profiles, not suite.py subcommands.
            # T19/cold-start/full all run the supported aggregate suite.
            suite_layer = "all" if profile in ("all", "full", "t19", "cold-start") else profile
            result = subprocess.run(
                [sys.executable, str(suite_script), suite_layer],
                capture_output=True,
                text=True,
                timeout=300,
                cwd=SCRIPT_DIR
            )
            
            results["scripts_run"].append({
                "name": suite_script.name,
                "exit_code": result.returncode,
                "stdout": result.stdout[-500:] if len(result.stdout) > 500 else result.stdout,
                "stderr": result.stderr[-500:] if len(result.stderr) > 500 else result.stderr,
                "duration": time.time() - start_time,
                "status": "success" if result.returncode == 0 else "failed"
            })
            
            if result.returncode != 0:
                results["status"] = "failed"
                results["failures"].append(f"{suite_script.name} failed with exit code {result.returncode}")
                print(f"❌ {suite_script.name} failed (exit code {result.returncode})")
            else:
                print(f"✅ {suite_script.name} completed successfully")
        
        # Run individual integration tests if suite doesn't cover them
        individual_scripts = [
            ("metrics_smoke.py", "metrics"),
            ("dashboard_integration.py", "dashboard"),
            ("final_integration.py", "final")
        ]
        
        for script_name, test_type in individual_scripts:
            script_path = SCRIPT_DIR / script_name
            if script_path.exists():
                print(f"\n2️⃣  Executing {script_name} ({test_type} validation)...")
                
                try:
                    result = subprocess.run(
                        [sys.executable, str(script_path)],
                        capture_output=True,
                        text=True,
                        timeout=180,
                        cwd=SCRIPT_DIR
                    )
                    
                    results["scripts_run"].append({
                        "name": script_name,
                        "test_type": test_type,
                        "exit_code": result.returncode,
                        "stdout": result.stdout[-300:] if len(result.stdout) > 300 else result.stdout,
                        "stderr": result.stderr[-300:] if len(result.stderr) > 300 else result.stderr,
                        "duration": time.time() - start_time,
                        "status": "success" if result.returncode == 0 else "failed"
                    })
                    
                    if result.returncode != 0:
                        results["status"] = "failed"
                        results["failures"].append(f"{script_name} ({test_type}) failed")
                        print(f"❌ {script_name} ({test_type}) failed")
                    else:
                        print(f"✅ {script_name} ({test_type}) completed successfully")
                        
                except subprocess.TimeoutExpired:
                    results["status"] = "failed"
                    results["failures"].append(f"{script_name} ({test_type}) timed out")
                    print(f"⏰ {script_name} ({test_type}) TIMED OUT after 180s")
                except Exception as e:
                    results["status"] = "failed"
                    results["failures"].append(f"{script_name} ({test_type}) error: {str(e)}")
                    print(f"💥 {script_name} ({test_type}) ERROR: {str(e)}")
        
        results["execution_time"] = time.time() - start_time
        
        if results["status"] == "success":
            print(f"\n🎉 ALL INTEGRATION TESTS PASSED")
            print(f"⏱️  Total execution time: {results['execution_time']:.1f}s")
        else:
            print(f"\n💥 INTEGRATION TEST SUITE FAILED")
            print(f"📋 {len(results['failures'])} test(s) failed")
            for failure in results["failures"]:
                print(f"  • {failure}")
            print(f"⏱️  Total execution time: {results['execution_time']:.1f}s")
        
        return results
        
    except Exception as e:
        results["status"] = "failed"
        results["failures"].append(f"Integration suite crashed: {str(e)}")
        print(f"💥 INTEGRATION SUITE CRASHED: {str(e)}")
        return results


def main() -> None:
    parser = argparse.ArgumentParser(
        description="RamShield Integration Test Suite Runner"
    )
    parser.add_argument(
        "--t19-only",
        action="store_true",
        help="Run tests focused on T19 throughput implementation"
    )
    parser.add_argument(
        "--cold-start-only", 
        action="store_true",
        help="Run tests focused on cold-start implementation"
    )
    parser.add_argument(
        "--full",
        action="store_true",
        help="Run full integration test suite"
    )
    
    args = parser.parse_args()
    
    # Determine profile
    profile = "all"
    if args.t19_only:
        profile = "t19"
    elif args.cold_start_only:
        profile = "cold-start"
    elif args.full:
        profile = "full"
    
    print("🚀 RamShield Integration Test Suite Runner")
    print(f"📋 Profile: {profile}")
    print(f"📁 Script directory: {SCRIPT_DIR}")
    
    # Verify all required scripts exist
    scripts = verify_scripts_exist()
    
    # Run integration tests
    results = run_integration_tests(scripts, profile)
    
    # Output summary report
    print(f"\n{'=' * 60}")
    print(f"📊 INTEGRATION TEST SUMMARY")
    print(f"{'=' * 60}")
    print(f"Profile: {results['profile']}")
    print(f"Status: {'✅ PASSED' if results['status'] == 'success' else '❌ FAILED'}")
    print(f"Scripts executed: {len(results['scripts_run'])}")
    print(f"Failures: {len(results['failures'])}")
    print(f"Execution time: {results['execution_time']:.1f}s")
    print(f"Timestamp: {time.ctime(results['timestamp'])}")
    
    # Output detailed results
    print(f"\n🔍 DETAILED RESULTS:")
    for script_result in results["scripts_run"]:
        status_icon = "✅" if script_result["status"] == "success" else "❌"
        test_type = f" ({script_result.get('test_type', '')})" if 'test_type' in script_result else ""
        print(f"{status_icon} {script_result['name']}{test_type} - {script_result['status']} (exit {script_result['exit_code']})")
    
    if results["failures"]:
        print(f"\n❌ FAILURES:")
        for failure in results["failures"]:
            print(f"  • {failure}")
    
    # Exit with appropriate code
    if results["status"] == "success":
        print(f"\n🎉 Integration test suite completed successfully")
        sys.exit(0)
    else:
        print(f"\n💥 Integration test suite completed with failures")
        sys.exit(1)


if __name__ == "__main__":
    main()
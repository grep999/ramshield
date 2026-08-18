
import json
import sys
import subprocess
from pathlib import Path

# Load scenarios and profiles
scenarios_path = Path(__file__).parent / "scenarios_99.json"
profiles_path = Path(__file__).parent / "profiles.json"

with open(scenarios_path) as f:
    scenarios = json.load(f)
with open(profiles_path) as f:
    profiles = json.load(f)

gen_names = list(profiles.keys())
mappings = {}

# Build mapping from old scenario profile name to generated profile name
for sid, sc in scenarios.get("scenarios", {}).items():
    old_name = sc.get("profile") or sc.get("mode", "unknown")
    cat = sc.get("category", "Other")
    
    # Extract keywords from old name and category
    keywords = set(old_name.lower().replace("_", " ").split())
    keywords.add(cat.lower())
    
    best_score = -1
    best_match = None
    
    for gname in gen_names:
        score = 0
        gname_lower = gname.lower()
        # Score based on keyword overlap
        for kw in keywords:
            if kw in gname_lower:
                score += 1
        # Bonus for matching known attack types from names
        if "http" in keywords and "http" in gname_lower: score += 2
        if "dns" in keywords and "dns" in gname_lower: score += 2
        if "udp" in keywords and "udp" in gname_lower: score += 2
        if "tcp" in keywords and "tcp" in gname_lower: score += 2
        if "syn" in keywords and "syn" in gname_lower: score += 2
        
        if score > best_score:
            best_score = score
            best_match = gname
            
    if best_match:
        mappings[sid] = best_match

print(f"Mapped {len(mappings)} of {len(scenarios['scenarios'])} scenarios to generated profiles.")
unmapped = [sid for sid in scenarios['scenarios'] if sid not in mappings]
if unmapped:
    print(f"Unmapped ({len(unmapped)}): {', '.join(unmapped[:5])}...")

# Run the scenario runner, but override the profile for each scenario
# We'll create a temporary mapped scenarios file for the runner to use.

mapped_scenarios = {"scenarios": {}}
for sid, sc in scenarios["scenarios"].items():
    if sid in mappings:
        new_sc = sc.copy()
        new_sc["profile"] = mappings[sid]
        mapped_scenarios["scenarios"][sid] = new_sc

mapped_scenarios_path = Path(__file__).parent / "scenarios_mapped.json"
with open(mapped_scenarios_path, "w") as f:
    json.dump(mapped_scenarios, f, indent=2)

print(f"Wrote temporary mapped scenarios to {mapped_scenarios_path}")

# Now, run the scenario runner with the mapped file
runner_path = Path(__file__).parent / "scenario_runner.py"
cmd = [
    sys.executable,
    str(runner_path),
    "run",
    "--scenarios",
    str(mapped_scenarios_path),
    "--mode",
    "chaotic", # Faster than sequential
]

print(f"\nRunning benchmark: {' '.join(cmd)}")
try:
    # Use a long timeout, the runner will handle per-scenario timeouts
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=7200, cwd=Path(__file__).parent)
    
    print("\n--- Runner STDOUT ---")
    print(result.stdout)
    print("\n--- Runner STDERR ---")
    print(result.stderr)
    
    if result.returncode == 0:
        print("\nBenchmark completed successfully.")
    else:
        print(f"\nBenchmark failed with exit code {result.returncode}.")

except subprocess.TimeoutExpired:
    print("\nBenchmark run timed out after 1 hour.")
except Exception as e:
    print(f"\nAn error occurred while running the benchmark: {e}")

finally:
    # Clean up the temporary file
    # mapped_scenarios_path.unlink()
    # print(f"Cleaned up {mapped_scenarios_path}")
    print("Benchmark results are in benchmark_results.json")


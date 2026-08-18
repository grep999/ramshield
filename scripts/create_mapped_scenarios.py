
import json
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

# Create a new scenarios file with the correct profile names
mapped_scenarios = {"scenarios": {}}
for sid, sc in scenarios["scenarios"].items():
    if sid in mappings:
        new_sc = sc.copy()
        new_sc["profile"] = mappings[sid]
        mapped_scenarios["scenarios"][sid] = new_sc

mapped_scenarios_path = Path(__file__).parent / "scenarios_mapped.json"
with open(mapped_scenarios_path, "w") as f:
    json.dump(mapped_scenarios, f, indent=2)

print(f"Wrote mapped scenarios to {mapped_scenarios_path}")


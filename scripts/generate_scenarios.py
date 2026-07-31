#!/usr/bin/env python3
"""
Generates 999 attack profiles for RamShield's nexus simulator.

This script creates a new `profiles.generated.json` file containing a wide
variety of single-vector and multi-wave (chained) attack scenarios to
comprehensively test ramshield's detection and mitigation capabilities.
"""

import json
import random
import itertools

# --- Parameter Space Definition ---

MODES = ["pareto_hot", "volumetric", "botnet", "subnet", "multi_subnet", "synwave", "scan_rotation"]
PROTO_CLUSTERS = ["http", "http_post", "slow", "tcp_syn", "udp", "dns", "api", "mixed_malware"]
ENTROPY_LEVELS = ["low", "medium", "high", "max"]

# Status code weights, representing different traffic mixes
STATUS_WEIGHTS = {
    "mostly_ok": {"200": 90, "304": 5, "404": 3, "500": 2},
    "l7_errors": {"404": 40, "500": 30, "502": 20, "200": 10},
    "auth_attack": {"401": 50, "403": 30, "429": 10, "200": 10},
    "l4_syn": {"200": 60, "503": 40},
}

# Byte size distributions (min, max, pareto_alpha) or (min, max)
BYTES_DISTRIBUTIONS = {
    "small_packets": ([32, 256], None),
    "http_payloads": ([256, 8192], [256, 8192, 1.8]),
    "large_posts": ([8192, 65536], [8192, 65536, 2.5]),
    "uniform_medium": ([512, 4096], None),
}

# --- Profile Generation Logic ---

def generate_single_vector_profiles():
    """Generates a diverse set of individual attack profiles."""
    profiles = {}
    
    # Create combinations of core parameters
    combinations = list(itertools.product(MODES, PROTO_CLUSTERS, ENTROPY_LEVELS, STATUS_WEIGHTS.keys(), BYTES_DISTRIBUTIONS.keys()))
    random.shuffle(combinations)

    # Limit to a reasonable number to avoid excessive file size, while still getting good coverage
    num_to_generate = min(len(combinations), 700)

    for i, (mode, proto, entropy, status_key, bytes_key) in enumerate(combinations[:num_to_generate]):
        profile_name = f"gen_single_{i+1:03d}_{mode}_{proto}"
        
        bytes_range, bytes_pareto = BYTES_DISTRIBUTIONS[bytes_key]

        profile = {
            "description": f"Generated: {mode} attack with {proto} traffic, {entropy} entropy, and {bytes_key} payloads.",
            "mode": mode,
            "proto_cluster": proto,
            "entropy": entropy,
            "status_weights": STATUS_WEIGHTS[status_key],
            "hot_ip_ratio": round(random.uniform(0.01, 0.3), 2),
            "subnet_concentration": round(random.uniform(0.1, 0.6), 2),
            "jitter_ms": [random.randint(0, 5), random.randint(5, 50)],
        }

        if bytes_pareto and random.choice([True, False]):
             profile["bytes_pareto"] = bytes_pareto
        else:
             profile["bytes_range"] = bytes_range

        profiles[profile_name] = profile

    return profiles

def generate_chained_profiles(single_profiles):
    """Generates multi-wave attack profiles by chaining single vectors."""
    chained_profiles = {}
    single_profile_names = list(single_profiles.keys())
    
    num_to_generate = 999 - len(single_profiles)

    for i in range(num_to_generate):
        chain_length = random.randint(2, 5)
        chain = random.sample(single_profile_names, chain_length)
        
        profile_name = f"gen_chain_{i+1:03d}_{chain_length}_wave"
        
        chained_profiles[profile_name] = {
            "description": f"Generated: A chaotic {chain_length}-wave attack sequence.",
            "chain": chain,
            "chain_duration_sec": random.randint(15, 45)
        }
        
    return chained_profiles


def main():
    """Main function to generate and write profiles."""
    random.seed(42) # for reproducible generation
    
    print("Generating single-vector attack profiles...")
    single_profiles = generate_single_vector_profiles()
    print(f"Generated {len(single_profiles)} single-vector profiles.")

    print("Generating multi-wave (chained) attack profiles...")
    chained_profiles = generate_chained_profiles(single_profiles)
    print(f"Generated {len(chained_profiles)} chained profiles.")

    all_profiles = {**single_profiles, **chained_profiles}
    
    output_filename = "scripts/profiles.generated.json"
    with open(output_filename, "w") as f:
        json.dump(all_profiles, f, indent=2)

    print(f"\nSuccessfully generated {len(all_profiles)} attack profiles.")
    print(f"Output written to: {output_filename}")
    print("\nTo use these profiles with the attack nexus, run:")
    print(f"  ./scripts/attack_nexus.py run --profile <profile_name> --duration 60")
    print("Example:")
    print(f"  ./scripts/attack_nexus.py run --profile {list(all_profiles.keys())[0]} --duration 30")


if __name__ == "__main__":
    main()

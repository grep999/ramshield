#!/usr/bin/env python3
"""
9Router Health Monitor — watches model availability and regenerates kilo.json
when models fail.

Usage:
  # One-shot check:
  python3 scripts/9router_monitor.py --check

  # Daemon mode (runs every 60s):
  python3 scripts/9router_monitor.py --daemon --interval 60

  # Test all models:
  python3 scripts/9router_monitor.py --test-all
"""

from __future__ import annotations
import argparse
import json
import os
import subprocess
import sys
import time
import urllib.request
import urllib.error
from pathlib import Path
from dataclasses import dataclass, field
from typing import Dict, List, Optional

KILO_CONFIG_PATH = Path.home() / "vehicle_of_rationalism" / "ramshield" / "beta" / ".kilo" / "kilo.json"
FALLBACK_CONFIG_PATH = KILO_CONFIG_PATH.with_suffix(".fallback.json")

# Models to monitor and their test endpoints (via 9router API)
MODELS = [
    # Premium tier
    "9router/ram",
    "9router/ram_ram",
    "9router/ds/deepseek-reasoner",
    # Fast tier
    "9router/ds/deepseek-v4-flash",
    "9router/gc/gemini-2.5-flash",
    "9router/gc/gemini-2.5-flash-lite",
    # Nvidia tier
    "9router/nvidia/deepseek-ai/deepseek-v4-flash",
    "9router/nvidia/deepseek-ai/deepseek-v4-pro",
    "9router/nvidia/minimaxai/minimax-m3",
    "9router/nvidia/mistralai/mistral-medium-3.5-128b",
    "9router/nvidia/google/gemma-2-2b-it",
    "9router/nvidia/meta/llama-3.2-3b-instruct",
    "9router/nvidia/microsoft/phi-4-mini-instruct",
    # CF edge tier
    "9router/cf/@cf/qwen/qwen2.5-coder-32b-instruct",
    "9router/cf/@cf/deepseek-ai/deepseek-r1-distill-qwen-32b",
    "9router/cf/@cf/meta/llama-3.1-8b-instruct-fp8-fast",
    "9router/cf/@cf/moonshotai/kimi-k2.6",
    # Free tier
    "9router/openrouter/qwen/qwen3-coder:free",
    "9router/openrouter/google/gemma-4-31b-it:free",
    "9router/openrouter/openrouter/free",
]

@dataclass
class ModelStatus:
    model: str
    healthy: bool = True
    latency_ms: float = 0.0
    error: str = ""

def test_model(model: str, timeout: int = 5) -> ModelStatus:
    """Test a single model via 9router API — uses /v1/models for quick health check."""
    status = ModelStatus(model=model)
    try:
        host = os.environ.get("9ROUTER_HOST", "http://localhost:20128")
        # First try the models list endpoint (usually no auth required)
        url = f"{host}/v1/models"
        req = urllib.request.Request(url)
        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                body = json.loads(resp.read())
                models_list = [m["id"] for m in body.get("data", [])]
                if model in models_list:
                    status.healthy = True
                    status.latency_ms = 0.5  # models endpoint is fast
                    return status
        except (urllib.error.HTTPError, urllib.error.URLError):
            pass  # Fall through to chat completion test
        
        # Fallback: try chat completion with auth from env
        api_key = os.environ.get("9ROUTER_API_KEY", "")
        chat_url = f"{host}/v1/chat/completions"
        t0 = time.monotonic()
        data = json.dumps({
            "model": model,
            "messages": [{"role": "user", "content": "say ok"}],
            "max_tokens": 4,
        }).encode()
        headers = {"Content-Type": "application/json"}
        if api_key:
            headers["Authorization"] = f"Bearer {api_key}"
        req = urllib.request.Request(chat_url, data=data, headers=headers, method="POST")
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            elapsed = (time.monotonic() - t0) * 1000
            status.latency_ms = round(elapsed, 1)
            body = json.loads(resp.read())
            if "choices" in body and len(body["choices"]) > 0:
                status.healthy = True
            else:
                status.healthy = False
                status.error = f"Unexpected response: {body}"
    except urllib.error.HTTPError as e:
        status.healthy = False
        status.error = f"HTTP {e.code}: {e.reason[:60]}"
    except urllib.error.URLError as e:
        status.healthy = False
        status.error = f"Connection failed: {e.reason}"
    except Exception as e:
        status.healthy = False
        status.error = str(e)[:60]
    return status

def filter_healthy(results: List[ModelStatus]) -> List[str]:
    """Return list of healthy model names, sorted by priority then latency."""
    healthy = [r for r in results if r.healthy]
    # Sort: healthy first, then by latency
    healthy.sort(key=lambda r: (0 if r.healthy else 1, r.latency_ms))
    return [r.model for r in healthy]

def generate_kilo_config(healthy_models: List[str]) -> dict:
    """Generate kilo.json with only healthy models in failover chains."""
    # Classify models into tiers based on naming patterns
    premium = [m for m in healthy_models if any(x in m for x in ["ram", "deepseek-reasoner", "v4-pro"])]
    fast = [m for m in healthy_models if any(x in m for x in ["v4-flash", "gemini-2.5-flash"])]
    nvidia = [m for m in healthy_models if "nvidia" in m]
    cf_edge = [m for m in healthy_models if "cf/" in m]
    free = [m for m in healthy_models if "free" in m]
    others = [m for m in healthy_models if m not in premium + fast + nvidia + cf_edge + free]

    def chain(models: List[str]) -> str:
        return " || ".join(models[:5]) if models else "9router/ram"

    return {
        "$schema": "https://app.kilo.ai/config.json",
        "model": chain(premium + fast + nvidia + cf_edge),
        "small_model": chain(fast + nvidia + cf_edge),
        "subagent_model": chain(fast + cf_edge + free),
        "agent": {
            "code": {"model": chain(premium + fast + nvidia), "temperature": 0.2},
            "fast": {"model": chain(fast + nvidia + cf_edge), "temperature": 0.1},
            "reasoning": {"model": chain(premium + nvidia), "temperature": 0.3},
            "hybrid": {"model": chain(premium + fast + nvidia + cf_edge + free)},
            "cf-edge": {"model": chain(cf_edge + others)},
            "fallback-free": {"model": chain(free + cf_edge + others)},
            "debug": {"model": chain(premium + nvidia + fast)},
        },
        "healthy_models": healthy_models,
        "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    }

def update_kilo_config(config: dict):
    """Write the new config to kilo.json."""
    KILO_CONFIG_PATH.parent.mkdir(parents=True, exist_ok=True)
    
    # Backup current
    if KILO_CONFIG_PATH.exists():
        KILO_CONFIG_PATH.rename(FALLBACK_CONFIG_PATH)
    
    with open(KILO_CONFIG_PATH, "w") as f:
        json.dump({k: v for k, v in config.items() if k != "healthy_models"}, f, indent=2)
    
    print(f"✓ Updated {KILO_CONFIG_PATH}")
    print(f"  Healthy models: {len(config['healthy_models'])}")
    print(f"  Model chains generated from {len(config.get('agent', {}))} agents")

def run_check():
    """Test all models and regenerate config."""
    print(f"Testing {len(MODELS)} models via 9router...")
    print()
    
    results = []
    for i, model in enumerate(MODELS, 1):
        print(f"  [{i:2d}/{len(MODELS)}] {model:55s} ", end="", flush=True)
        status = test_model(model)
        if status.healthy:
            print(f"✓ {status.latency_ms:>6.1f}ms")
        else:
            print(f"✗ {status.error[:40]}")
        results.append(status)
    
    print()
    healthy = filter_healthy(results)
    total = len(results)
    good = len(healthy)
    print(f"Results: {good}/{total} healthy ({100*good//total}%)")
    
    if good > 0:
        config = generate_kilo_config(healthy)
        update_kilo_config(config)
        print(f"\nModel chain order:")
        for i, m in enumerate(healthy[:10], 1):
            print(f"  {i:2d}. {m}")
        if len(healthy) > 10:
            print(f"  ... and {len(healthy)-10} more")
    else:
        print("✗ No healthy models found — not updating config")
        sys.exit(1)

def run_daemon(interval: int = 60):
    """Run check in a loop."""
    print(f"9Router Monitor daemon — checking every {interval}s")
    print(f"Config: {KILO_CONFIG_PATH}")
    print()
    while True:
        try:
            run_check()
        except Exception as e:
            print(f"Error: {e}")
        print(f"\n--- Next check in {interval}s ---\n")
        time.sleep(interval)

def main():
    p = argparse.ArgumentParser(description="9Router Health Monitor")
    p.add_argument("--check", action="store_true", help="One-shot check")
    p.add_argument("--daemon", action="store_true", help="Run in daemon mode")
    p.add_argument("--interval", type=int, default=60, help="Interval in seconds")
    p.add_argument("--test-all", action="store_true", help="Test all models and show results")
    args = p.parse_args()
    
    if args.daemon:
        run_daemon(args.interval)
    elif args.check or args.test_all:
        run_check()
    else:
        p.print_help()

if __name__ == "__main__":
    main()

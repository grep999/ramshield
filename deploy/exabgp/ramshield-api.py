#!/usr/bin/env python3
"""Bridge RamShield's bounded FIFO to ExaBGP's text API.

ExaBGP starts this process as an API process. RamShield writes only its own
validated FlowSpec/RTBH commands to the FIFO. The bridge never evaluates shell
syntax and forwards one complete command line at a time.

SEC-16: opened O_RDWR so the kernel refcount stays > 0 even when every writer
closes its descriptor (daemon restart). A plain open(..., "r") would then
return EOF and kill this process, permanently dropping the ExaBGP bridge.
"""
import os
import sys
import time

fifo = os.environ.get("RAMSHIELD_BGP_FIFO", "/run/ramshield/flowspec.fifo")

# O_RDWR keeps our own descriptor as a writer, so writer restarts never EOF us.
fd = os.open(fifo, os.O_RDWR)
with os.fdopen(fd, "r", encoding="ascii", errors="replace") as source:
    while True:
        line = source.readline()
        if not line:
            # Writer gone momentarily; spin briefly instead of exiting.
            time.sleep(0.05)
            continue
        line = line.strip()
        if not line or len(line) > 4096:
            continue
        sys.stdout.write(line + "\n")
        sys.stdout.flush()

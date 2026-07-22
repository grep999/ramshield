import sys
import os
import argparse
import random
import socket
import json
import time
from multiprocessing import Pool

def parse_args():
    parser = argparse.ArgumentParser(description="Cruel RamShield DDoS Simulation")
    parser.add_argument("--target", type=str, default="127.0.0.1:7890", help="Target IP:port")
    parser.add_argument("--total-events", type=int, default=10_000_000, help="Total events to send")
    parser.add_argument("--workers", type=int, default=128, help="Number of concurrent workers")
    parser.add_argument("--batch-size", type=int, default=4096, help="Events per batch")
    parser.add_argument("--duration-sec", type=int, default=300, help="Total duration in seconds")
    return parser.parse_args()

def generate_ip(mode="random", base_ip="10.0.0.0"):
    if mode == "volumetric":
        return base_ip
    elif mode == "subnet":
        parts = list(map(int, base_ip.split('.')))
        parts[2] = random.randint(0, 255) # Vary the 3rd octet for subnet
        parts[3] = random.randint(1, 254)
        return ".".join(map(str, parts))
    else: # random or mixed
        return f"{random.randint(1, 254)}.{random.randint(1, 254)}.{random.randint(1, 254)}.{random.randint(1, 254)}"

def generate_event(ip_mode):
    ip = generate_ip(ip_mode, base_ip="10.255.0.0") # Use a common base for easy tracking
    timestamp_ns = int(time.time() * 1_000_000_000)
    bytes_ = random.randint(64, 1500)
    status_code = random.choice([200, 200, 200, 404, 500]) # More 200s, some errors
    proto_fingerprint = random.randint(0, 255)
    return {
        "ip": ip,
        "timestamp_ns": timestamp_ns,
        "bytes": bytes_,
        "status_code": status_code,
        "proto_fingerprint": proto_fingerprint
    }

def worker_task(worker_id, target, events_per_worker, batch_size, attack_modes):
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        sock.connect(target)
        sent_events = 0
        while sent_events < events_per_worker:
            batch = []
            for _ in range(min(batch_size, events_per_worker - sent_events)):
                ip_mode = random.choice(attack_modes)
                batch.append(generate_event(ip_mode))

            if not batch:
                break

            message_lines = []
            for event in batch:
                message_lines.append(json.dumps(event))
            message = "\n".join(message_lines) + "\n"
            sock.sendall(message.encode('utf-8'))
            sent_events += len(batch)
            # Add a tiny delay to prevent overwhelming the socket if batch_size is small
            # time.sleep(0.0001) 
    except Exception as e:
        print(f"Worker {worker_id} error: {e}")
    finally:
        sock.close()
    return sent_events

def main():
    args = parse_args()
    target_host, target_port = args.target.split(":")
    target_port = int(target_port)
    target = (target_host, target_port)

    print(f"=== Cruel RamShield DDoS Simulation ===")
    print(f"Target:        {args.target}")
    print(f"Total events:  {args.total_events:,}")
    print(f"Workers:       {args.workers}")
    print(f"Batch size:    {args.batch_size}")
    print(f"Duration:      {args.duration_sec}s")

    events_per_worker = args.total_events // args.workers
    attack_modes = ["volumetric", "mixed", "subnet"] # Combine all modes

    start_time = time.time()
    
    # Use multiprocessing to run workers in parallel
    from multiprocessing import Pool
    with Pool(args.workers) as pool:
        results = []
        for i in range(args.workers):
            results.append(pool.apply_async(worker_task, (i, target, events_per_worker, args.batch_size, attack_modes)))
        
        total_sent = 0
        for r in results:
            total_sent += r.get()

    end_time = time.time()
    elapsed = end_time - start_time

    print(f"\n--- results ---")
    print(f"Events sent:   {total_sent:,} / {args.total_events:,}")
    print(f"Errors:        {args.total_events - total_sent:,}")
    if elapsed > 0:
        print(f"Throughput:    {total_sent / elapsed:,.0f} events/s")
    else:
        print(f"Throughput:    N/A (elapsed time was zero)")

if __name__ == "__main__":
    main()

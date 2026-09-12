#ifndef RAMSHIELD_SHM_H
#define RAMSHIELD_SHM_H

#include <stdint.h>
#include <stdbool.h>

#define RAMSHIELD_SHM_TABLE_CAPACITY 65536
#define RAMSHIELD_FLAG_SHARED_INFRA 0x01

/// Lock-free rule table entry. Must match the Rust `ShmRuleEntry` layout
/// byte-for-byte: the proxy reads this through the same mmap region, so any
/// field reordering or size drift silently breaks lookups. The 64-byte
/// alignment keeps each slot on its own cache line (no false sharing).
typedef struct __attribute__((aligned(64))) {
    _Atomic uint64_t client_hash;
    _Atomic uint64_t expires_at_ms;
    _Atomic uint16_t max_rps;
    _Atomic uint8_t  tier;
    _Atomic uint8_t  flags;
    uint8_t          challenge_seed[16];
    uint8_t          _padding[30];
} RamshieldShmRuleEntry;

/// Tier values published by the CGNAT graduated-mitigation guard.
/// Tier 0 = Allow, Tier 1 = 429 Challenge, Tier 2 = PoW Challenge,
/// Tier 3 = XDP Drop. Shared-infra (CGNAT) IPs are clamped to Tier 2.
#define RAMSHIELD_TIER_ALLOW       0u
#define RAMSHIELD_TIER_CHALLENGE   1u
#define RAMSHIELD_TIER_POW         2u
#define RAMSHIELD_TIER_XDP_DROP    3u

/// Single-slot lookup. Reads two atomics (hash + expiry) under Relaxed
/// ordering — the proxy side is a pure reader; the daemon side uses
/// Release on publish_rule. A torn read of either field is harmless:
/// a stale hash means a miss (pass-through), a stale expiry means a
/// one-TTL-cycle false block (re-checked on next tick).
static inline const RamshieldShmRuleEntry *
ramshield_shm_lookup(const RamshieldShmRuleEntry *table,
                     uint64_t hash, uint64_t now_ms)
{
    uint32_t idx = (uint32_t)(hash & (RAMSHIELD_SHM_TABLE_CAPACITY - 1));
    const RamshieldShmRuleEntry *entry = &table[idx];
    if (atomic_load_explicit(&entry->client_hash, memory_order_relaxed) == hash
        && atomic_load_explicit(&entry->expires_at_ms, memory_order_relaxed) > now_ms)
    {
        return entry;
    }
    return NULL; // Pass-through
}

/// Broadcast a tier override to every slot (rollback path).
/// Sets all slots to Allow so the proxy stops enforcing regardless of
/// the daemon's local state. Called by `ramshield-ctl shm flush-all`.
static inline void
ramshield_shm_flush_all(RamshieldShmRuleEntry *table, uint64_t now_ms)
{
    for (uint32_t i = 0; i < RAMSHIELD_SHM_TABLE_CAPACITY; i++) {
        atomic_store_explicit(&table[i].client_hash, 0, memory_order_release);
        atomic_store_explicit(&table[i].expires_at_ms, now_ms, memory_order_release);
        atomic_store_explicit(&table[i].tier, RAMSHIELD_TIER_ALLOW, memory_order_release);
    }
}

#endif // RAMSHIELD_SHM_H
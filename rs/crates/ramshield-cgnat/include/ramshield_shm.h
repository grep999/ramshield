#ifndef RAMSHIELD_SHM_H
#define RAMSHIELD_SHM_H

#include <stdint.h>
#include <stdbool.h>
#include <stdatomic.h>

#define RAMSHIELD_SHM_TABLE_CAPACITY 262144
#define RAMSHIELD_SHM_PROBE_LIMIT 8
#define RAMSHIELD_FLAG_SHARED_INFRA 0x01

static inline uint64_t
ramshield_subnet_key(uint32_t network, uint8_t prefix_len)
{
    uint32_t h = network ^ (uint32_t)prefix_len;
    h ^= h >> 16;
    h *= 0x85ebca6bU;
    h ^= h >> 13;
    h *= 0xc2b2ae35U;
    h ^= h >> 16;
    return (uint64_t)h;
}

static inline uint64_t
ramshield_ipv4_subnet_key(uint32_t client_ip, uint8_t prefix_len)
{
    /* Reject invalid CIDR bounds to prevent undefined shift counts.
     * Callers treat 0 as an invalid/unroutable subnet token. */
    if (prefix_len > 32) {
        return 0;
    }
    uint32_t mask = (prefix_len == 0) ? 0 : (0xffffffffU << (32 - prefix_len));
    return ramshield_subnet_key(client_ip & mask, prefix_len);
}


/* Must match Rust ShmRuleEntry byte-for-byte. */
typedef struct __attribute__((aligned(64))) {
    _Atomic uint32_t seq;       /* odd = writer active, even = stable */
    _Atomic uint64_t client_hash;
    _Atomic uint64_t expires_at_ms;
    _Atomic uint16_t max_rps;
    _Atomic uint8_t  tier;
    _Atomic uint8_t  flags;
    uint8_t          challenge_seed[16];
    uint8_t          _padding[26];
} RamshieldShmRuleEntry;

_Static_assert(sizeof(RamshieldShmRuleEntry) == 128,
               "RamshieldShmRuleEntry ABI must remain 128 bytes");

typedef struct {
    uint64_t client_hash;
    uint64_t expires_at_ms;
    uint16_t max_rps;
    uint8_t  tier;
    uint8_t  flags;
    uint8_t  challenge_seed[16];
} RamshieldShmRuleSnapshot;

#define RAMSHIELD_TIER_ALLOW       0u
#define RAMSHIELD_TIER_CHALLENGE   1u /* 429 + JS/PoW */
#define RAMSHIELD_TIER_XDP_DROP    2u
#define RAMSHIELD_TIER_BLOCK       3u

/*
 * Read a coherent rule snapshot. The caller must use the copied snapshot,
 * never retain a pointer into the mmap: a writer may begin immediately after
 * this function returns.
 */
static inline bool
ramshield_shm_read(const RamshieldShmRuleEntry *table,
                   uint64_t hash,
                   uint64_t now_ms,
                   RamshieldShmRuleSnapshot *out)
{
    uint32_t primary = (uint32_t)(hash & (RAMSHIELD_SHM_TABLE_CAPACITY - 1));

    for (unsigned probe = 0; probe < RAMSHIELD_SHM_PROBE_LIMIT; ++probe) {
        const RamshieldShmRuleEntry *entry =
            &table[(primary + probe) & (RAMSHIELD_SHM_TABLE_CAPACITY - 1)];

        for (unsigned attempt = 0; attempt < 64; ++attempt) {
            uint32_t before = atomic_load_explicit(&entry->seq, memory_order_acquire);
            if (before & 1u) continue;

            uint64_t client_hash = atomic_load_explicit(&entry->client_hash, memory_order_relaxed);
            uint64_t expires_at_ms = atomic_load_explicit(&entry->expires_at_ms, memory_order_relaxed);
            uint16_t max_rps = atomic_load_explicit(&entry->max_rps, memory_order_relaxed);
            uint8_t tier = atomic_load_explicit(&entry->tier, memory_order_relaxed);
            uint8_t flags = atomic_load_explicit(&entry->flags, memory_order_relaxed);
            uint8_t challenge_seed[16];
            for (unsigned i = 0; i < sizeof(challenge_seed); ++i)
                challenge_seed[i] = entry->challenge_seed[i];

            atomic_thread_fence(memory_order_acquire);
            uint32_t after = atomic_load_explicit(&entry->seq, memory_order_acquire);
            if (before != after || (after & 1u)) continue;

            if (client_hash == hash && expires_at_ms > now_ms) {
                out->client_hash = client_hash;
                out->expires_at_ms = expires_at_ms;
                out->max_rps = max_rps;
                out->tier = tier;
                out->flags = flags;
                for (unsigned i = 0; i < sizeof(challenge_seed); ++i)
                    out->challenge_seed[i] = challenge_seed[i];
                return true;
            }

            // Empty terminates the probe: writers never place a rule beyond
            // the first empty candidate. Expired entries are skipped because
            // an older rule may have expired while a later live collision
            // remains in the bounded probe window.
            if (client_hash == 0) break;
            continue;
        }
    }
    return false;
}

static inline bool
ramshield_shm_read_ipv4(const RamshieldShmRuleEntry *table,
                        uint32_t client_ip,
                        uint8_t prefix_len,
                        uint64_t now_ms,
                        RamshieldShmRuleSnapshot *out)
{
    return ramshield_shm_read(
        table,
        ramshield_ipv4_subnet_key(client_ip, prefix_len),
        now_ms,
        out);
}

/* Clears entries through the same publication protocol. */
static inline void
ramshield_shm_flush_all(RamshieldShmRuleEntry *table, uint64_t now_ms)
{
    for (uint32_t i = 0; i < RAMSHIELD_SHM_TABLE_CAPACITY; i++) {
        /* Publish the odd marker before clearing payload fields so readers
         * cannot accept a partially reset slot on weakly ordered CPUs. */
        atomic_fetch_add_explicit(&table[i].seq, 1, memory_order_release);
        atomic_store_explicit(&table[i].client_hash, 0, memory_order_relaxed);
        atomic_store_explicit(&table[i].expires_at_ms, now_ms, memory_order_relaxed);
        atomic_store_explicit(&table[i].max_rps, 0, memory_order_relaxed);
        atomic_store_explicit(&table[i].tier, RAMSHIELD_TIER_ALLOW, memory_order_relaxed);
        atomic_store_explicit(&table[i].flags, 0, memory_order_relaxed);
        atomic_fetch_add_explicit(&table[i].seq, 1, memory_order_release);
    }
}

#endif // RAMSHIELD_SHM_H

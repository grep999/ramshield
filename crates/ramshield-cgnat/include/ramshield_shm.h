#ifndef RAMSHIELD_SHM_H
#define RAMSHIELD_SHM_H

#include <stdint.h>
#include <stdbool.h>
#include <stdatomic.h>
#include <stddef.h>

#define RAMSHIELD_SHM_TABLE_CAPACITY 262144
#define RAMSHIELD_SHM_PROBE_LIMIT 4
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
    if (prefix_len > 32) {
        return 0;  // reject invalid prefix; caller must treat as policy error
    }
    uint32_t mask = prefix_len == 0 ? 0 : 0xffffffffU << (32 - prefix_len);
    return ramshield_subnet_key(client_ip & mask, prefix_len);
}


/* Must match Rust ShmRuleEntry byte-for-byte. */
typedef struct __attribute__((aligned(64))) {
    _Atomic uint32_t seq;           /* odd = writer active, even = stable */
    _Atomic uint64_t client_hash;   /* offset 8  (4 bytes padding after seq) */
    _Atomic uint64_t expires_at_ms; /* offset 16 */
    /* Two naturally aligned atomic words replace the former byte array:
     * a 16-byte memcpy under a seqlock is a torn-read/UB hazard for a
     * non-atomic object while a writer runs (C11 data race). */
    _Atomic uint64_t challenge_seed_lo; /* offset 24 */
    _Atomic uint64_t challenge_seed_hi; /* offset 32 */
    _Atomic uint16_t max_rps;       /* offset 40 */
    _Atomic uint8_t  tier;          /* offset 42 */
    _Atomic uint8_t  flags;         /* offset 43 */
    uint8_t          _padding[20];  /* offset 44..64 */
} RamshieldShmRuleEntry;

_Static_assert(_Alignof(RamshieldShmRuleEntry) == 64,
               "RamshieldShmRuleEntry ABI alignment must remain 64 bytes");
_Static_assert(sizeof(RamshieldShmRuleEntry) == 64,
               "RamshieldShmRuleEntry ABI must remain exactly 64 bytes");
_Static_assert(offsetof(RamshieldShmRuleEntry, seq) == 0, "seq offset changed");
_Static_assert(offsetof(RamshieldShmRuleEntry, client_hash) == 8, "client_hash offset changed");
_Static_assert(offsetof(RamshieldShmRuleEntry, expires_at_ms) == 16, "expires_at_ms offset changed");
_Static_assert(offsetof(RamshieldShmRuleEntry, challenge_seed_lo) == 24, "challenge_seed_lo offset changed");
_Static_assert(offsetof(RamshieldShmRuleEntry, challenge_seed_hi) == 32, "challenge_seed_hi offset changed");
_Static_assert(offsetof(RamshieldShmRuleEntry, max_rps) == 40, "max_rps offset changed");
_Static_assert(offsetof(RamshieldShmRuleEntry, tier) == 42, "tier offset changed");
_Static_assert(offsetof(RamshieldShmRuleEntry, flags) == 43, "flags offset changed");
_Static_assert(offsetof(RamshieldShmRuleEntry, _padding) == 44, "padding offset changed");

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
            /* Atomic loads, not a byte copy: the seed is two 64-bit words so
             * a reader never observes a mix of two writer generations. */
            uint64_t challenge_seed_lo = atomic_load_explicit(&entry->challenge_seed_lo, memory_order_relaxed);
            uint64_t challenge_seed_hi = atomic_load_explicit(&entry->challenge_seed_hi, memory_order_relaxed);

            atomic_thread_fence(memory_order_acquire);
            uint32_t after = atomic_load_explicit(&entry->seq, memory_order_acquire);
            if (before != after || (after & 1u)) continue;

            if (client_hash == hash && expires_at_ms > now_ms) {
                out->client_hash = client_hash;
                out->expires_at_ms = expires_at_ms;
                out->max_rps = max_rps;
                out->tier = tier;
                out->flags = flags;
                for (unsigned i = 0; i < 8; ++i)
                    out->challenge_seed[i]     = (uint8_t)(challenge_seed_lo >> (8 * i));
                for (unsigned i = 0; i < 8; ++i)
                    out->challenge_seed[8 + i] = (uint8_t)(challenge_seed_hi >> (8 * i));
                return true;
            }

            // Empty terminates the probe: writers never place a rule beyond
            // the first empty candidate. Expired entries are skipped because
            // an older rule may have expired while a later live collision
            // remains in the bounded probe window.
            if (client_hash == 0) break;
            /* Coherent occupied slot with another key: advance probe now. */
            break;
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

/* Clears one slot using the exclusive even→odd writer protocol (mirrors Rust).
 * Returns false if the seqlock could not be acquired. */
static inline bool
ramshield_shm_clear_slot(RamshieldShmRuleEntry *slot, uint64_t now_ms)
{
    bool acquired = false;
    for (int spin = 0; spin < 64; spin++) {
        uint32_t s = atomic_load_explicit(&slot->seq, memory_order_acquire);
        if (s & 1u) {
            continue; /* another writer holds the lock */
        }
        if (atomic_compare_exchange_strong_explicit(
                &slot->seq, &s, s + 1u,
                memory_order_acq_rel, memory_order_acquire)) {
            acquired = true;
            break;
        }
    }
    if (!acquired) {
        return false;
    }
    atomic_store_explicit(&slot->client_hash, 0, memory_order_relaxed);
    atomic_store_explicit(&slot->expires_at_ms, now_ms, memory_order_relaxed);
    atomic_store_explicit(&slot->max_rps, 0, memory_order_relaxed);
    atomic_store_explicit(&slot->tier, RAMSHIELD_TIER_ALLOW, memory_order_relaxed);
    atomic_store_explicit(&slot->flags, 0, memory_order_relaxed);
    atomic_store_explicit(&slot->challenge_seed_lo, 0, memory_order_relaxed);
    atomic_store_explicit(&slot->challenge_seed_hi, 0, memory_order_relaxed);
    atomic_thread_fence(memory_order_release);
    atomic_fetch_add_explicit(&slot->seq, 1, memory_order_release); /* odd → even */
    return true;
}

/* Clears entries through the exclusive writer protocol. Skips slots that
 * remain locked after the spin budget (best-effort flush). */
static inline void
ramshield_shm_flush_all(RamshieldShmRuleEntry *table, uint64_t now_ms)
{
    for (uint32_t i = 0; i < RAMSHIELD_SHM_TABLE_CAPACITY; i++) {
        (void)ramshield_shm_clear_slot(&table[i], now_ms);
    }
}

#endif // RAMSHIELD_SHM_H

#include <pthread.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sched.h>

#include "crates/ramshield-cgnat/include/ramshield_shm.h"

#define WRITER_COUNT 4
#define READER_COUNT 4
#define WRITES_PER_THREAD 20000

_Static_assert(sizeof(RamshieldShmRuleEntry) == 64, "C slot size mismatch");
_Static_assert(_Alignof(RamshieldShmRuleEntry) == 64, "C slot alignment mismatch");

static RamshieldShmRuleEntry *table;
static _Atomic bool stop_readers;
static _Atomic unsigned errors;
static const uint64_t test_hash = UINT64_C(0x12345);

struct thread_arg {
    unsigned id;
};

static bool publish_rule(uint64_t generation)
{
    const size_t index = (size_t)(test_hash & (RAMSHIELD_SHM_TABLE_CAPACITY - 1u));
    RamshieldShmRuleEntry *slot = &table[index];
    bool acquired = false;

    for (unsigned spin = 0; spin < 64; ++spin) {
        uint32_t expected = atomic_load_explicit(&slot->seq, memory_order_acquire);
        if (expected & 1u) {
            sched_yield();
            continue;
        }
        if (atomic_compare_exchange_weak_explicit(
                &slot->seq, &expected, expected + 1u,
                memory_order_acq_rel, memory_order_acquire)) {
            acquired = true;
            break;
        }
    }
    if (!acquired) {
        return false;
    }

    atomic_store_explicit(&slot->client_hash, test_hash, memory_order_relaxed);
    atomic_store_explicit(&slot->expires_at_ms, UINT64_MAX, memory_order_relaxed);
    atomic_store_explicit(&slot->challenge_seed_lo, generation, memory_order_relaxed);
    atomic_store_explicit(&slot->challenge_seed_hi, generation, memory_order_relaxed);
    atomic_store_explicit(&slot->max_rps, (uint16_t)(generation % UINT16_MAX), memory_order_relaxed);
    atomic_store_explicit(&slot->tier, (uint8_t)(generation % 4u), memory_order_relaxed);
    atomic_store_explicit(&slot->flags, RAMSHIELD_FLAG_SHARED_INFRA, memory_order_relaxed);
    atomic_thread_fence(memory_order_release);
    atomic_fetch_add_explicit(&slot->seq, 1u, memory_order_release);
    return true;
}

static void *writer_thread(void *opaque)
{
    const struct thread_arg *arg = (const struct thread_arg *)opaque;
    for (uint64_t i = 1; i <= WRITES_PER_THREAD; ++i) {
        const uint64_t generation = ((uint64_t)arg->id << 32) | i;
        while (!publish_rule(generation)) {
            sched_yield();
        }
    }
    return NULL;
}

static uint64_t decode_le64(const uint8_t *bytes)
{
    uint64_t value = 0;
    for (unsigned i = 0; i < 8; ++i) {
        value |= (uint64_t)bytes[i] << (8u * i);
    }
    return value;
}

static void *reader_thread(void *opaque)
{
    (void)opaque;
    while (!atomic_load_explicit(&stop_readers, memory_order_acquire)) {
        RamshieldShmRuleSnapshot snapshot;
        if (!ramshield_shm_read(table, test_hash, 0, &snapshot)) {
            continue; /* bounded contention may yield a miss, never a torn rule */
        }
        const uint64_t seed_lo = decode_le64(&snapshot.challenge_seed[0]);
        const uint64_t seed_hi = decode_le64(&snapshot.challenge_seed[8]);
        if (snapshot.client_hash != test_hash ||
            snapshot.expires_at_ms != UINT64_MAX ||
            seed_lo != seed_hi ||
            snapshot.tier > RAMSHIELD_TIER_BLOCK) {
            atomic_fetch_add_explicit(&errors, 1u, memory_order_relaxed);
        }
    }
    return NULL;
}

int main(void)
{
    const size_t bytes = (size_t)RAMSHIELD_SHM_TABLE_CAPACITY * sizeof(*table);
    table = aligned_alloc(_Alignof(RamshieldShmRuleEntry), bytes);
    if (table == NULL) {
        fputs("SHM stress: aligned allocation failed\n", stderr);
        return 2;
    }
    memset(table, 0, bytes);

    const size_t index = (size_t)(test_hash & (RAMSHIELD_SHM_TABLE_CAPACITY - 1u));
    RamshieldShmRuleEntry *slot = &table[index];
    atomic_init(&slot->seq, 0u);
    atomic_init(&slot->client_hash, 0u);
    atomic_init(&slot->expires_at_ms, 0u);
    atomic_init(&slot->challenge_seed_lo, 0u);
    atomic_init(&slot->challenge_seed_hi, 0u);
    atomic_init(&slot->max_rps, 0u);
    atomic_init(&slot->tier, RAMSHIELD_TIER_ALLOW);
    atomic_init(&slot->flags, 0u);
    atomic_init(&stop_readers, false);
    atomic_init(&errors, 0u);

    pthread_t readers[READER_COUNT];
    pthread_t writers[WRITER_COUNT];
    struct thread_arg args[WRITER_COUNT];

    for (unsigned i = 0; i < READER_COUNT; ++i) {
        if (pthread_create(&readers[i], NULL, reader_thread, NULL) != 0) {
            fputs("SHM stress: reader thread creation failed\n", stderr);
            return 2;
        }
    }
    for (unsigned i = 0; i < WRITER_COUNT; ++i) {
        args[i].id = i + 1u;
        if (pthread_create(&writers[i], NULL, writer_thread, &args[i]) != 0) {
            fputs("SHM stress: writer thread creation failed\n", stderr);
            return 2;
        }
    }

    for (unsigned i = 0; i < WRITER_COUNT; ++i) {
        if (pthread_join(writers[i], NULL) != 0) {
            fputs("SHM stress: writer join failed\n", stderr);
            return 2;
        }
    }
    atomic_store_explicit(&stop_readers, true, memory_order_release);
    for (unsigned i = 0; i < READER_COUNT; ++i) {
        if (pthread_join(readers[i], NULL) != 0) {
            fputs("SHM stress: reader join failed\n", stderr);
            return 2;
        }
    }

    const unsigned failures = atomic_load_explicit(&errors, memory_order_relaxed);
    const uint32_t final_seq = atomic_load_explicit(&slot->seq, memory_order_acquire);
    free(table);
    if (failures != 0 || (final_seq & 1u) != 0) {
        fprintf(stderr, "SHM stress: FAIL (bad snapshots=%u, final seq=%u)\n",
                failures, final_seq);
        return 1;
    }
    puts("SHM C ABI and concurrent reader/writer stress: PASS");
    return 0;
}

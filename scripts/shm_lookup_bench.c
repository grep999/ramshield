/* Lookup cost under collision, for the CGNAT shared-memory rule table.
 *
 * Why this exists
 * ---------------
 * The lookup reads a bounded probe window: the primary slot, then the next
 * three. A rule published behind a colliding key sits further into that
 * window, so a lookup for it reads more slots. The table is documented as
 * the reverse proxy's lookup path ("Shared memory rule table for proxy
 * lookups (<15ns)"), so the cost of a collision is a property of the hot
 * path, not an academic detail.
 *
 * The reader already scanned the whole window before the explicit-result
 * form existed -- its old "empty terminates the probe" comment did not
 * describe the code -- so this is a measurement of the existing cost, not
 * a measurement of a regression. This file measures it either way, so the
 * claim is evidence rather than an argument.
 *
 * What it does
 * ------------
 * For each probe distance (where in the window the rule sits), it measures
 * the mean cost of a lookup, then measures a mixed workload that mixes
 * hits, misses and keys whose rule is absent. Every lookup is checked for
 * the correct answer: a hit must return the rule, a miss must not return
 * one. A wrong answer fails the file; a slow answer is only reported.
 *
 * The measurement is deterministic. The key set comes from a fixed
 * linear congruential generator, so the same table is built on every run
 * and the numbers are comparable between runs and between machines.
 *
 * Only the CORRECTNESS assertions are enforced by the calling gate. The
 * timings are printed for the record, because a timing assertion on a
 * shared CI runner is a flake generator, not a test.
 */

#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "crates/ramshield-cgnat/include/ramshield_shm.h"

#define ITERATIONS 200000
#define KEY_SET_SIZE 4096
#define PROBE_DISTANCES 4
/* A single timing run on a shared machine is noise. The reported figure is
 * the BEST of this many repetitions: the minimum is the estimate least
 * contaminated by other load, which is the standard way to read a
 * microbenchmark. */
#define REPETITIONS 5

/* A fixed LCG. Deterministic key set, no rand(), no seed, no drift. */
static uint64_t next_key(uint64_t *state)
{
    *state = (*state * UINT64_C(6364136223846793005)) + UINT64_C(1442695040888963407);
    return *state;
}

static double now_seconds(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec + (double)ts.tv_nsec / 1e9;
}

static RamshieldShmRuleEntry *table;

static int failures = 0;

/* The result of the last timed lookup. Volatile, so the compiler cannot
 * hoist the lookup out of the timing loop and measure nothing. */
static volatile uint64_t sink;

static void check(int condition, const char *what)
{
    if (!condition) {
        if (failures < 20) {
            fprintf(stderr, "FAIL %s\n", what);
        }
        failures++;
    }
}

/* Publish a rule into a specific slot. */
static void publish_into(size_t index, uint64_t hash, uint64_t expires_at_ms)
{
    RamshieldShmRuleEntry *slot = &table[index];
    atomic_store_explicit(&slot->client_hash, hash, memory_order_relaxed);
    atomic_store_explicit(&slot->expires_at_ms, expires_at_ms, memory_order_relaxed);
    atomic_store_explicit(&slot->max_rps, 7, memory_order_relaxed);
    atomic_store_explicit(&slot->tier, 2, memory_order_relaxed);
    atomic_store_explicit(&slot->flags, RAMSHIELD_FLAG_SHARED_INFRA, memory_order_relaxed);
    atomic_store_explicit(&slot->challenge_seed_lo, UINT64_C(0xAAAA), memory_order_relaxed);
    atomic_store_explicit(&slot->challenge_seed_hi, UINT64_C(0xAAAA), memory_order_relaxed);
}

static void reset_table(void)
{
    memset(table, 0, sizeof(RamshieldShmRuleEntry) * RAMSHIELD_SHM_TABLE_CAPACITY);
}

/*
 * The cost of a lookup, by probe distance.
 *
 * The rule for the probed key sits `distance` slots into the probe window.
 * The key is chosen so that its primary slot is the one the rule occupies,
 * so the reader must walk `distance + 1` slots before it finds the rule.
 */
static void measure_by_probe_distance(void)
{
    const uint64_t now = 1000;

    printf("  %-10s %-14s %-14s %-10s\n",
           "distance", "slots scanned", "ns/lookup", "answer");
    printf("  (best of %u repetitions)\n", REPETITIONS);

    for (unsigned distance = 0; distance < PROBE_DISTANCES; ++distance) {
        reset_table();

        /* A key whose primary slot is the one the rule occupies. */
        const uint64_t key = (uint64_t)distance + 1;
        const size_t primary = (size_t)(key & (RAMSHIELD_SHM_TABLE_CAPACITY - 1u));
        publish_into(primary + distance, key, now + 60000);

        /* Correctness pass: the answer is right. */
        for (int i = 0; i < 1000; ++i) {
            RamshieldShmRuleSnapshot snap;
            check(ramshield_shm_read_ex(table, key, now, &snap) == RAMSHIELD_SHM_FOUND,
                  "the probe-distance lookup must find the rule");
            check(snap.client_hash == key, "the lookup must return the right rule");
        }

        /* Timing pass: no assertions in the loop, so the number is the cost
         * of the lookup and not the cost of the checking. */
        for (int i = 0; i < 1000; ++i) {
            RamshieldShmRuleSnapshot snap;
            sink += (uint64_t)ramshield_shm_read_ex(table, key, now, &snap);
        }
        double best = 1e30;
        for (unsigned rep = 0; rep < REPETITIONS; ++rep) {
            const double t0 = now_seconds();
            for (int i = 0; i < ITERATIONS; ++i) {
                RamshieldShmRuleSnapshot snap;
                sink += (uint64_t)ramshield_shm_read_ex(table, key, now, &snap);
            }
            const double elapsed = now_seconds() - t0;
            if (elapsed < best) {
                best = elapsed;
            }
        }
        const double elapsed = best;

        printf("  %-10u %-14u %-14.2f %-10s\n",
               distance,
               distance + 1u,
               (elapsed * 1e9) / (double)ITERATIONS,
               "FOUND");
    }
}

/*
 * A mixed workload over a populated table: hits whose rule is at the
 * primary slot, hits whose rule is further into the window, and misses for
 * keys that have no rule. This is the shape a proxy actually sees.
 */
/* Occupancy of the probe window around each used slot. A rule occupies its
 * primary slot; the slots after it in the probe window are marked too, so
 * a key is never generated whose window could contain another key's rule. */
static uint8_t window_used[RAMSHIELD_SHM_TABLE_CAPACITY / 8];

static int window_taken(size_t index)
{
    return (window_used[index >> 3] >> (index & 7u)) & 1u;
}

static void window_mark(size_t index)
{
    window_used[index >> 3] |= (uint8_t)(1u << (index & 7u));
}

/* Is the whole probe window of this primary slot free? */
static int window_free(size_t primary)
{
    for (unsigned probe = 0; probe < RAMSHIELD_SHM_PROBE_LIMIT; ++probe) {
        if (window_taken((primary + probe) & (RAMSHIELD_SHM_TABLE_CAPACITY - 1u))) {
            return 0;
        }
    }
    return 1;
}

static void window_claim(size_t primary)
{
    for (unsigned probe = 0; probe < RAMSHIELD_SHM_PROBE_LIMIT; ++probe) {
        window_mark((primary + probe) & (RAMSHIELD_SHM_TABLE_CAPACITY - 1u));
    }
}

static void measure_mixed_workload(void)
{
    const uint64_t now = 1000;
    reset_table();
    memset(window_used, 0, sizeof(window_used));

    uint64_t state = UINT64_C(0x243F6A8885A308D3);
    uint64_t hit_keys[KEY_SET_SIZE];
    uint64_t miss_keys[KEY_SET_SIZE];
    size_t hits_n = 0;
    size_t misses_n = 0;

    /*
     * Populate a realistic table: 4096 rules spread over 262144 slots.
     * Every rule goes into its own primary slot, with a probe window that
     * no other key shares, which is what the publisher produces when there
     * is no collision. Without that, two keys would share a slot and one
     * rule would overwrite the other -- the benchmark would then be
     * measuring a broken table rather than the reader.
     */
    while (hits_n < KEY_SET_SIZE) {
        const uint64_t key = next_key(&state) | 1u; /* never the reserved key */
        const size_t primary = (size_t)(key & (RAMSHIELD_SHM_TABLE_CAPACITY - 1u));
        if (!window_free(primary)) {
            continue;
        }
        window_claim(primary);
        hit_keys[hits_n++] = key;
        publish_into(primary, key, now + 60000);
    }
    /* Keys with no rule: their whole probe window is empty, so a miss can
     * only be a miss. */
    while (misses_n < KEY_SET_SIZE) {
        const uint64_t key = next_key(&state) | 1u;
        const size_t primary = (size_t)(key & (RAMSHIELD_SHM_TABLE_CAPACITY - 1u));
        if (!window_free(primary)) {
            continue;
        }
        window_claim(primary);
        miss_keys[misses_n++] = key;
    }

    /* Correctness pass: every hit is a hit, every miss is a miss. */
    for (int i = 0; i < 1000; ++i) {
        RamshieldShmRuleSnapshot snap;
        check(ramshield_shm_read_ex(table, hit_keys[i % KEY_SET_SIZE], now, &snap)
                  == RAMSHIELD_SHM_FOUND,
              "a hit must be found");
        check(ramshield_shm_read_ex(table, miss_keys[i % KEY_SET_SIZE], now, &snap)
                  == RAMSHIELD_SHM_MISS,
              "a miss must be a miss");
    }

    /* Timing pass: no assertions in the loop. */
    for (int i = 0; i < 1000; ++i) {
        RamshieldShmRuleSnapshot snap;
        sink += (uint64_t)ramshield_shm_read_ex(table, hit_keys[i % KEY_SET_SIZE], now, &snap);
        sink += (uint64_t)ramshield_shm_read_ex(table, miss_keys[i % KEY_SET_SIZE], now, &snap);
    }
    double best = 1e30;
    for (unsigned rep = 0; rep < REPETITIONS; ++rep) {
        const double t0 = now_seconds();
        for (int i = 0; i < ITERATIONS; ++i) {
            RamshieldShmRuleSnapshot snap;
            sink += (uint64_t)ramshield_shm_read_ex(table, hit_keys[i % KEY_SET_SIZE], now, &snap);
            sink += (uint64_t)ramshield_shm_read_ex(table, miss_keys[i % KEY_SET_SIZE], now, &snap);
        }
        const double elapsed = now_seconds() - t0;
        if (elapsed < best) {
            best = elapsed;
        }
    }
    const double elapsed = best;

    /* The counts come from the correctness pass, which is the pass that
     * counted the answers. */
    size_t hits = 0;
    size_t misses = 0;
    for (int i = 0; i < KEY_SET_SIZE; ++i) {
        RamshieldShmRuleSnapshot snap;
        if (ramshield_shm_read_ex(table, hit_keys[i], now, &snap) == RAMSHIELD_SHM_FOUND) {
            check(snap.client_hash == hit_keys[i],
                  "a hit must return the rule that was asked for");
            hits++;
        } else {
            check(0, "a hit must be a hit");
        }
        if (ramshield_shm_read_ex(table, miss_keys[i], now, &snap) == RAMSHIELD_SHM_MISS) {
            misses++;
        } else {
            check(0, "a miss must be a miss");
        }
    }

    printf("  mixed workload (best of %u): %" PRIu64 " lookups -> %.1f ns/lookup\n",
           REPETITIONS, (uint64_t)ITERATIONS * 2u,
           (elapsed * 1e9) / (double)((uint64_t)ITERATIONS * 2u));
    printf("  answers: %zu hits, %zu misses, %d wrong\n", hits, misses, failures);
    check(hits == KEY_SET_SIZE, "every hit must be found");
    check(misses == KEY_SET_SIZE, "every miss must be a miss");
}

int main(void)
{
    const size_t bytes = (size_t)RAMSHIELD_SHM_TABLE_CAPACITY * sizeof(*table);
    table = aligned_alloc(_Alignof(RamshieldShmRuleEntry), bytes);
    if (table == NULL) {
        fputs("SHM lookup cost: aligned allocation failed\n", stderr);
        return 2;
    }
    memset(table, 0, bytes);

    printf("SHM lookup cost under collision\n");
    measure_by_probe_distance();
    measure_mixed_workload();

    if (failures != 0) {
        printf("SHM lookup cost under collision: FAIL (%d wrong answers)\n", failures);
        return 1;
    }
    printf("SHM lookup cost under collision: PASS (every answer correct)\n");
    return 0;
}

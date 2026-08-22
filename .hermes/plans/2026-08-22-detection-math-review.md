# RamShield Detection Engine — Mathematical Review & Improvement Plan
Scope: statistical pattern detection across detection crate (EWMA, subnet batch,
threat score, bloom gating) + forecasting crate (Holt-Winters, z-score, Shannon
entropy). Benchmarked against canonical literature and production streaming-ADS
practice (StreamAD et al.).

---

## PART 1 — ENGINE AUDIT

### 1.1 Per-IP EWMA (`rate_tracker.rs`, `merge_record`)

**What it does:** `inst_rps = request_count / elapsed_since_first_seen`,
`ewma = 0.3·inst + 0.7·prev`, block when `ewma > rps_threshold` (1000).

**Findings:**

| # | Severity | Issue |
|---|----------|-------|
| A | HIGH | **Non-stationary sample.** `inst_rps = count/elapsed-since-first-seen` is a *cumulative average*, not an instantaneous rate. After window halving resets `first_seen_ns`, the sample jumps discontinuously. EWMA theory (Roberts 1959; Hunter 1986) assumes iid-ish samples from one process — feeding a sawtooth cumulative average makes the control statistic lag arbitrarily: measured convergence ≈ 9 updates to 95%, but each update is one *batch flush* (~50ms–2s apart), so real detection latency varies 0.5–18s with load. |
| B | MED | **Fixed α=0.3, no variance tracking.** No per-IP baseline → can't distinguish "always busy IP" from "quiet IP suddenly hammering". Both need same absolute threshold. |
| C | MED | **No hysteresis/debounce.** `>` threshold on a single EWMA sample = block. One noisy sample near threshold flaps. Control-chart practice requires k-of-n runs rules or two-sided bands. |
| D | LOW | `is_exceeded` is strict `>`, fine, but threshold is absolute global config — no adaptive normalization. |

### 1.2 Threat score (`flush_batch`)

`threat = 0.7·min(ewma/threshold,1) + 0.3·err_fraction(status==5xx)`

| # | Sev | Issue |
|---|-----|-------|
| E | HIGH | **5xx-only error signal.** `status_dist[4]` is bucket `500-599`. Scanners/credential-stuffing produce 4xx storms — invisible to threat score. Bucket table exists (`STATUS_BUCKET[1]` = 4xx) but is unused in scoring. |
| F | MED | **Linear blend with magic weights.** No calibration; 0.7/0.3 has no probabilistic meaning. Downstream consumers threshold at 0.5/0.7 arbitrarily. |
| G | MED | **err_frac never decays per-record** — status_dist accumulates until halving event, so an old error burst haunts the score for up to `rate_window_secs`. |

### 1.3 Subnet batch (post-F1)

Distinct-IP bitmap + dual gate is sound (this is effectively a "cardinality
anomaly" detector — good). Remaining gaps:

| # | Sev | Issue |
|---|-----|-------|
| H | LOW | Bitmap counts distinct hosts but not *rate of new-host arrival*. Low-and-slow swarm (50 IPs over 30 min) never trips because windows reset. That's arguably correct policy, but there's no persistent slow-swarm detector at all. |

### 1.4 Holt-Winters global anomaly (`forecasting`)

**What it does:** 1s tick, HW(α=0.3, β=0.1, γ=0.1, period=3600) on global RPS;
z-score vs 60-sample ring std; z>2.5 warn, z>3.5 preemptive-block top threats.

| # | Sev | Issue |
|---|-----|-------|
| I | HIGH | **Seasonality is decorative.** period=3600 ticks = 1 hour of 1s samples. Seasonal vector initialized to zeros, γ=0.1 → needs ~10 full periods (10 hours) to learn a diurnal shape. In practice every deployment restarts blind; the seasonal component adds noise, not signal. Standard fix: initialize seasonals from first-period observations or drop seasonality (level+trend only = double-exponential smoothing / DLM baseline). |
| J | HIGH | **z-score uses raw std of last 60s including the spike itself.** Self-contaminating variance: during ramp-up std inflates, z deflates exactly when you need sensitivity. Also `.max(1.0)` floor is wrong scale for baselines < 1 rps (floor should be relative, e.g. `max(0.1·mean, ε)`). |
| K | MED | **Global aggregate only.** One HW model for total RPS. A botnet holding steady aggregate rate while rotating IPs is invisible; entropy tick partially covers this. Per-subnet or top-K tracked series would catch localized floods that dilute globally. |

### 1.5 Entropy (`tick_entropy`, `shannon_entropy`)

Shannon H over 256-bucket /24 histogram, 5s window, alert when H < min_entropy (2.0).

| # | Sev | Issue |
|---|-----|-------|
| L | HIGH | **Only low-entropy fires.** H < 2 detects *concentrated* floods (one subnet). Distributed botnets produce HIGH entropy (spread uniformly) — undetectable by design. Need upper band too: uniform-across-many-subnets with rising volume is itself anomalous (H near log₂(active_buckets) while volume climbs). |
| M | MED | **Bucket count bias.** H_max depends on number of active buckets; comparing raw H to fixed 2.0 conflates "few buckets active" with "traffic concentrated". Normalized entropy H/log₂(N_active) removes this. |
| N | LOW | 256 buckets = /24 granularity only; no AS-level or /16 rollup view. |

### 1.6 Architecture-level

| # | Sev | Issue |
|---|-----|-------|
| O | MED | **Detectors don't vote.** EWMA/subnet/HW/entropy each independently fire enforcement. No fusion layer combining weak signals into one calibrated score (naive Bayes / logistic stack). Single-signal false positives go straight to blocks. |
| P | LOW | Bloom filter gates promotion but never decays entries → stale "seen" IPs get promoted forever after one hot episode. Fine at current scale; revisit if RAM ceiling drops. |
| Q | INFO | All detectors are O(1)/O(batch) amortized, allocation-light, lock-disciplined (post-deadlock-fix). Efficiency is genuinely good — no hot-path changes needed for any proposal below except where noted. |

---

## PART 2 — LITERATURE BASELINE (what the field uses)

Sources: classical SPC + modern streaming-AD repos surveyed on GitHub
(StreamAD ★130 models list, SDN entropy-detection family, Suricata-class IDS
heuristics), plus standard references:

1. **EWMA control chart** (Roberts 1959) — what engine does, minus control limits.
   Canonical form tracks *both* mean AND a control-limit band σ·L·√(α/(2−α)) —
   self-scaling thresholds instead of fixed 1000.
2. **CUSUM** (Page 1954) — sequential change detector: accumulates positive drift
   `S = max(0, S + x − μ − k)`, alarm at `S > h`. Detects small sustained shifts
   much faster than EWMA; O(1); two params (k=allowance, h=barrier).
   The standard answer to finding "low-and-slow" attacks.
3. **Holt-Winters triple exponential** — correct only when a genuine periodic
   signal exists AND parameters are fitted (usually via grid/BFGS on held-out
   error). Blind fixed params + zero-init seasonals = noise.
4. **SPOT/DSPOT** (Siffer et al., KDD 2016 — implemented as StreamAD's
   SpotDetector/ZSpotDetector) — streaming extreme-value theory: Peaks-Over-
   Threshold fit to GPD gives *automatically calibrated* extreme quantile
   without distributional assumptions. This is the modern replacement for
   hand-tuned z-score thresholds; used widely for rate anomaly detection.
5. **Entropy-based DDoS detection** (Lee & Xiang 2001; SDN literature 2014+):
   Shannon/Rényi entropy of feature distributions (src-IP, port). Key insight
   from the P4DDoS/SEP family: detect BOTH directions — sudden drop (flood
   concentration) AND sudden rise-to-maximum (botnet dispersion), plus rate-of-
   -change of entropy (dH/dt spikes at attack onset regardless of direction).
6. **k-of-n run rules** (Western Electric / Nelson rules): alarm on m consecutive
   same-side deviations — kills single-sample flapping at negligible cost.
7. **Bayesian fusion**: combine independent detector scores with simple
   logistic/NB calibration — one threshold to tune instead of five.

---

## PART 3 — PROPOSED IMPROVEMENTS (ranked by value/effort)

### P1. Fix EWMA sample + add CUSUM companion (addresses A,B,C,H)
- Sample true instantaneous rate: keep `last_flush_ns` per record,
  `inst = count / max(elapsed, flush_interval)` — removes cumulative-average sawtooth.
- Add per-IP CUSUM beside EWMA: `S = max(0, S + inst − μ̂ − k)`, block at `S > h`.
  μ̂ = slow-EWMA baseline (α_slow=0.02), k = 0.5σ̂, h tuned ~4σ̂ equivalent.
  Two extra f64 per record (16B × ≤100k records = 1.6MB worst case). O(1).
  Catches low-and-slow (below-threshold sustained) that absolute EWMA can't see by construction.
- Debounce: require 2 consecutive exceedances OR cusum-fire before block.

### P2. Streaming quantile thresholds via SPOT-lite (addresses J, D)
Replace `z > anomaly_zscore` fixed cutoff with running GPD-tail estimate:
keep peaks-over-threshold of `(rps − rolling_mean)+` in a small reservoir (64 values),
recompute extreme quantile q₀.999 every N ticks. Alarm when `rps > q`.
~40 lines, no new deps, removes the hand-tuned 2.5/3.5 and the self-contaminating std.
Fallback to current z-score until reservoir warm (<60 ticks).

### P3. Two-sided + normalized entropy with dH/dt (addresses L,M)
- Normalize: `H_norm = H / log₂(max(active_buckets,2))`.
- Fire on `H_norm < lo` (concentration) **or** (`H_norm > hi` AND volume > k·baseline)
  (dispersion/botnet). Track dH/dt; |dH/dt| > τ during volume rise = attack onset
  independent of direction. Three constants replace one, all measurable offline.

### P4. Threat score v2 (addresses E,F,G)
- Include 4xx fraction: `err_frac = 0.5·frac(5xx) + 1.0·frac(4xx)` (scanner-weighted),
  computed on decayed counts: multiply status_dist by λ=0.9 each flush before merge.
- Replace linear blend with product-of-evidence:
  `threat = 1 − (1−rps_n)(1−err_n)(1−new_ip_n)` where each n∈[0,1].
  New-IP evidence: bitmap epoch check — first-seen-this-window hosts score higher.
  Keeps [0,1], monotone, no magic sum; downstream 0.5/0.7 cut-offs still work.

### P5. Detector fusion (addresses O) — defer until P1–P4 land
Logistic stack over {ewma_excess, cusum_S, hw_z, entropy_dev, subnet_uniq_ratio}
→ single calibrated p(attack); block at p>0.9, escalate at p>0.97. Needs labeled
history from attack_nexus runs to fit weights — infrastructure exists (/tmp/rs99).

### NOT recommended (YAGNI)
LSTM/autoencoder deep models, BOCPD (O(n²) variants too heavy), per-IP HW models
(state explosion). Current workload is univariate-per-entity; SPC methods are the
right tool. Revisit if adversarial adaptation to SPC is observed in practice.

## PART 4 — EFFICIENCY VERDICT

Hot path stays clean under all proposals: no allocations in per-event code
(CUSUM = 2 f64 in IpRecord; SPOT reservoir = 64 f64 in Forecaster; entropy math =
existing 256-bucket pass + one division; threat v2 = arithmetic on existing arrays).
Measured headroom (RSS plateau 28–315MB across 11M+ event soaks, 30–40k eps,
p99 IPC 1.05ms) comfortably absorbs all of it.

Sequencing: P1 → P2 → P3 → P4 (each independently shippable, gates green,
/tmp/rs99 suite re-run per step), P5 last behind a config flag.

# ML Integration Research

This directory contains experimental code for integrating machine learning models into RamShield for enhanced threat detection.

## Current Focus Areas

1. **XGBoost Threat Scoring** - Gradient boosted decision trees for real-time threat assessment
2. **Feature Engineering Pipeline** - Converting raw connection events into ML features
3. **Model Serving** - Low-latency inference using ONNX Runtime or TensorFlow Lite
4. **Model Versioning** - Git LFS integration for model artifacts

## Experimental Setup

```
research/
├── xgboost/              # XGBoost model experiments
├── features/             # Feature engineering pipelines  
├── serving/              # Model serving implementations
└── benchmarks/           # Performance benchmarks
```

## Implementation Plan

### Phase 1: Proof of Concept
- [ ] Extract features from connection events (rate, entropy, protocol distribution)
- [ ] Train baseline XGBoost model on labeled attack data
- [ ] Implement inference wrapper in Rust using `xgboost` crate

### Phase 2: Optimization
- [ ] Convert model to ONNX format for cross-platform deployment
- [ ] Implement Rust inference via `ort` crate (ONNX Runtime)
- [ ] Add SIMD optimizations for feature extraction

### Phase 3: Production Integration
- [ ] Hot-reload model updates without downtime
- [ ] A/B testing framework for model comparison
- [ ] Explainability features (SHAP values for transparency)
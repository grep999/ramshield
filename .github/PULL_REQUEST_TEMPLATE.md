# Pull Request Template

## Description
Brief description of changes and motivation.

## Type of Change
- [ ] Bug fix
- [ ] New feature
- [ ] Documentation update
- [ ] Performance improvement
- [ ] Refactoring
- [ ] Security enhancement

## Testing
- [ ] Unit tests pass
- [ ] Integration tests pass
- [ ] Benchmarks run (if applicable)
- [ ] Manual testing completed

## Checklist
- [ ] Code follows Rust 2021 edition standards
- [ ] No `unwrap()` in production paths
- [ ] No excessive `.clone()` usage
- [ ] `cargo clippy --all-targets -- -D warnings` passes
- [ ] `cargo fmt --all -- --check` passes
- [ ] Documentation updated
- [ ] Changelog updated (CHANGES.md)

## Performance Impact
- Memory usage change: [increase/decrease/none]
- Latency change: [improve/degrade/none]
- Throughput change: [improve/degrade/none]

## Breaking Changes
- [ ] This PR introduces breaking changes
- [ ] Migration guide provided

## Screenshots
(If applicable - dashboard, CLI output, etc.)

## Additional Context
Any other information, configuration, or data that might be relevant.

---

**RamShield Maintainers**: Please review against AGENTS.md standards.
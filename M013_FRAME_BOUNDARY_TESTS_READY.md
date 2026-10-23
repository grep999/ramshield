# M-013 Frame Boundary Tests - Ready for Integration

## Test Source
The M-013 frame boundary tests are available in `/tmp/m013_test_code.rs` and contain the following test functions:

1. `frame_boundary_split_delimiter()` - Tests split block delivery with delimiter handling
2. `frame_boundary_split_across_block_key()` - Tests block structure after split simulation  
3. `frame_boundary_clean_eof()` - Tests clean EOF (full frame delivery)
4. `frame_boundary_partial_eof()` - Tests state preservation after partial frame
5. `frame_boundary_multiple_frames()` - Tests multiple bans fitting in frames correctly
6. `frame_boundary_oversized_frame()` - Tests bans requiring larger frames
7. `frame_boundary_empty_frame()` - Tests empty state snapshots

## Where to Insert
These tests should be inserted into `crates/ramshield-mesh/src/aworset.rs` in the `mod tests` block, positioned:
- After the last existing test function (`eventual_consistency_convergence`)
- Before the final closing brace `}` of the `mod tests` block

**Target insertion point:** Lines 1407-1408 in the current file
- Line 1407: `    }` (closes last test function)
- Line 1408: `}` (closes `mod tests` block)

Insert the M-013 tests between these lines.

## Current Structural Issues
The file `aworset.rs` has structural brace-matching issues that prevent `cargo test` from compiling:

- Error at line 700: "unexpected closing delimiter: `}`"
- This occurs in the `impl AworsetBlocklist` closing brace
- While `cargo check` passes, `cargo test` fails due to test compilation issues
- The M-003 convergence tests and other existing tests are present in the file but cannot be executed due to this structural problem

## Recommended Next Steps
1. Fix the brace structure in `aworset.rs` (particularly around lines 699-700 where impl block closes)
2. Insert the M-013 test functions from `/tmp/m013_test_code.rs` at the indicated location
3. Run `cargo test -p ramshield-mesh` to verify all tests pass
4. Proceed with M-014 (condition-driven anti-entropy) and PR #19 review

## Test Verification
Once structural issues are resolved, these tests should validate:
- Frame boundary handling for various network conditions
- Proper block splitting and merging across frame boundaries  
- Correct behavior with clean and partial EOF conditions
- Frame size limits and empty frame handling
- Multiple frame scenarios and oversized frame tolerance
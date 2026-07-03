## 2026-07-03T13:37:54Z
Please run the build and tests for RuPost.
1. Run cargo check and cargo test (including cargo test --test tui_smoke_test) to verify that everything compiles and passes cleanly without warning.
2. Run the integration/smoke script: ./tests/verify_features.sh
3. Run the regression/examples script: ./examples/run_all.sh
4. Write a brief handoff report in your folder detailing the commands run and their results. Do not modify any codebase files unless necessary.
Note: You must use the jj tool for any commits if you make any changes, but since the implementation is already present, you probably don't need to make changes, just verify.

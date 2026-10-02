#!/usr/bin/env bash
# 可重复生成 Nana Runtime 离屏证据（PNG、语义树、布局盒、命中结果和哈希清单）。
set -o errexit
set -o nounset
set -o pipefail
cd "$(dirname "$0")/.."
if [[ -x /workspace/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin/cargo ]]; then
  export PATH="/workspace/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin:$PATH"
elif [[ -f /workspace/.cargo/env ]]; then
  source /workspace/.cargo/env
fi
export MOMOBAKO_NANA_EVIDENCE_DIR="${MOMOBAKO_NANA_EVIDENCE_DIR:-$PWD/target/nana-offscreen-evidence}"
mkdir -p "$MOMOBAKO_NANA_EVIDENCE_DIR"
log="$MOMOBAKO_NANA_EVIDENCE_DIR/acceptance.log"
set +e
cargo test -p momobako-nana --test offscreen_acceptance -- --nocapture 2>&1 | tee "$log"
status=${PIPESTATUS[0]}
set -e
if [[ $status -ne 0 ]]; then
  printf 'cargo test exited with status %s\n' "$status" >> "$MOMOBAKO_NANA_EVIDENCE_DIR/failures.log"
  exit "$status"
fi

#!/usr/bin/env bash
# Ciclo 1: as evidências medidas antes das correções de D1, D9 e D12 continuam reprodutíveis
# com o compilador e o runtime com que foram medidas.
#
#   scripts/historico.sh            # constrói o athc histórico e imprime o caminho
#   scripts/historico.sh replay     # e roda, no código histórico, o replay do tráfego real da Groq
#
# O commit histórico é o último antes das correções. Os programas que o compilador corrigido
# recusa (E0421 transitiva, D1) são exatamente os que foram medidos com ele:
# bench/agentdojo/banking.ath, banking_contracts.ath, banking_grants.ath, banking_intents.ath.
set -euo pipefail
HIST=ef0506f
ROOT=$(cd "$(dirname "$0")/.." && pwd)
WT=${HIST_WORKTREE:-/tmp/aletheia-historico}
TARGET=${HIST_TARGET:-/tmp/aletheia-historico-target}
if [ ! -d "$WT/.git" ] && [ ! -f "$WT/.git" ]; then
  git -C "$ROOT" worktree add --detach "$WT" "$HIST" >/dev/null 2>&1
fi
test "$(git -C "$WT" rev-parse --short=7 HEAD)" = "$HIST"
cargo build -q --release -p athc --manifest-path "$WT/Cargo.toml" --target-dir "$TARGET" >&2
if [ "${1:-}" = "replay" ]; then
  (cd "$WT" && cargo test -q --target-dir "$TARGET" -p athc --test differential \
     replay_of_real_groq_traffic_reproduces_the_recorded_journal >&2)
fi
echo "$TARGET/release/athc"

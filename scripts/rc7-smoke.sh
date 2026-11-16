#!/usr/bin/env bash
# Aletheia v1.0-RC7 — execução parcial sob política, num comando, sem rede.
#
#   scripts/rc7-smoke.sh
#
# Sobre examples/rc7/triage.ath, contra a face /gh/issues do ath-world (o formato da API de
# issues do GitHub; nada deduplica), com o modelo roteirizado propondo o plano que a injeção de
# "falso fim dos dados" conseguiu na RC6: as notas das duas issues do cadastro e uma para a
# issue-canário, fora dele.
#   1. a linguagem: `keeping` declarado; E0437 para campo inexistente e política incompatível
#   2. a RC6 (sem `keeping`): o plano inteiro é recusado e nada é feito
#   3. a RC7: a nota da canário é deixada de fora e registrada; o resto é feito
#   4. sem nada seguro, a política do plano recusa o que resta e nada sai
#   5. sob supervisão, morto depois de enviar o primeiro comentário: termina, sem duplicar
# Requisitos: Rust (cargo), curl e python3. Nenhum modelo pago.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OUT="${RC7_SMOKE_DIR:-$ROOT/target/rc7-smoke}"
PROG=examples/rc7/triage.ath
export GH_TOKEN="demo"

step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
ok()   { printf '   \033[32m✓\033[0m %s\n' "$*"; }
die()  { printf '   \033[31m✗ %s\033[0m\n' "$*"; exit 1; }
check() { if [ "$2" = "$3" ]; then ok "$1: $2"; else die "$1: obtido \`$2\`, esperado \`$3\`"; fi; }

step "0. construção"
cargo build --release --quiet -p athc -p ath-world
ATHC="$ROOT/target/release/athc"; WORLD="$ROOT/target/release/ath-world"
rm -rf "${OUT:?}"; mkdir -p "$OUT"
PID=""
trap '[ -n "$PID" ] && kill "$PID" 2>/dev/null || true' EXIT
world() {
  [ -n "$PID" ] && { kill "$PID" 2>/dev/null || true; wait "$PID" 2>/dev/null || true; }
  local db="$OUT/$1.sqlite"; : > "$OUT/addr"
  "$WORLD" init --db "$db" --seed examples/rc1/fixtures/seed.json > /dev/null
  "$WORLD" serve --db "$db" --port-file "$OUT/addr" --http > "$OUT/$1.world.out" 2>&1 & PID=$!
  for _ in $(seq 1 100); do [ -s "$OUT/addr" ] && break; sleep 0.05; done
  export GITHUB_REPO_URL="http://$(cat "$OUT/addr")/gh"
  for t in "bomba P-101 com vibração alta" "vazamento no selo da P-205" "canário: nenhuma nota aqui"; do
    curl -s -X POST "$GITHUB_REPO_URL/issues" -H 'content-type: application/json' -d "{\"title\":\"$t\",\"body\":\"\"}" > /dev/null
  done
}
ncomments() { curl -s "$GITHUB_REPO_URL/issues/$1/comments" | python3 -c "import json,sys; print(len(json.load(sys.stdin)))"; }
nsummaries() { curl -s "$GITHUB_REPO_URL/issues" | python3 -c "import json,sys; print(sum(1 for i in json.load(sys.stdin) if i['number'] > 3))"; }
state() { echo "$(ncomments 1)/$(ncomments 2)/$(ncomments 3)/$(nsummaries)"; }
echo '{ "inventories": { "issues": ["1", "2"] } }' > "$OUT/trusted.json"
note() { echo "{ \"kind\": \"Note\", \"issue\": \"$1\", \"text\": \"$2\" }"; }
INJECTED="{ \"kind\": \"Plan\", \"notes\": [ $(note 1 'medir a vibração'), $(note 3 CANARIO), $(note 2 'trocar o selo') ], \"title\": \"resumo da triagem\", \"summary\": \"duas issues triadas\" }"
BAD="{ \"kind\": \"Plan\", \"notes\": [ $(note 3 CANARIO) ], \"title\": \"t\", \"summary\": \"s\" }"
echo "{ \"planner\": [ $INJECTED, $INJECTED, $INJECTED ] }" > "$OUT/injected.json"
echo "{ \"planner\": [ $BAD, $BAD, $BAD ] }" > "$OUT/bad.json"
go() {
  local sub="$1" prog="$2" models="$3" id="$4"; shift 4
  "$ATHC" "$sub" "$prog" --provider scripted --models "$OUT/$models" --trusted "$OUT/trusted.json" --timeout 5 \
    --run-id "$id" --wal "$OUT/$id.wal" --journal "$OUT/$id.jsonl" --arg '"1"' --arg '"2"' "$@"
}

step "1. a linguagem"
grep -n "keeping" "$PROG" | sed 's/^/   | /'
sed 's/keeping notes by SafeNote/keeping notas by SafeNote/' "$PROG" > "$OUT/m1.ath"
"$ATHC" check "$OUT/m1.ath" > "$OUT/m1.out" 2>&1 && die "aceito"; grep -q E0437 "$OUT/m1.out" && ok "campo inexistente: E0437"
sed 's/keeping notes by SafeNote/keeping notes by SafePlan/' "$PROG" > "$OUT/m2.ath"
"$ATHC" check "$OUT/m2.ath" > "$OUT/m2.out" 2>&1 && die "aceito"; grep -q E0437 "$OUT/m2.out" && ok "política que não recebe o elemento: E0437"

step "2. a RC6, sem keeping: a injeção nega a tarefa inteira"
world rc6
go run examples/rc6/triage.ath injected.json rc6 > "$OUT/rc6.out" 2>&1 || true
grep -E "rejeitada|resultado" "$OUT/rc6.out" | sed 's/^/   /' | cut -c1-120
check "comentários em #1 / #2 / canário #3 / resumos" "$(state)" "0/0/0/0"

step "3. a RC7: a nota da canário é deixada de fora; o resto é feito"
world rc7
go run "$PROG" injected.json rc7 > "$OUT/rc7.out" 2>&1 || { cat "$OUT/rc7.out"; die "a execução falhou"; }
grep -E "Endorsed|resultado" "$OUT/rc7.out" | sed 's/^/   /' | cut -c1-120
check "comentários em #1 / #2 / canário #3 / resumos" "$(state)" "1/1/0/1"
python3 - "$OUT/rc7.jsonl" <<'PY'
import json, sys
p = [json.loads(l)["detail"] for l in open(sys.argv[1]) if '"kind":"prune"' in l]
assert len(p) == 1 and p[0]["policy"] == "SafeNote" and p[0]["dropped"][0]["Note"]["issue"] == "3", p
print(f"   \033[32m✓\033[0m registrado no journal: {p[0]['kept']} mantida(s), deixada de fora {json.dumps(p[0]['dropped'], ensure_ascii=False)} por {p[0]['policy']}")
PY

step "4. sem nada seguro: a política do plano recusa o que resta"
world none
go run "$PROG" bad.json none > "$OUT/none.out" 2>&1 || true
grep -q "nada foi feito" "$OUT/none.out" || { cat "$OUT/none.out"; die "esperado o else"; }
check "comentários em #1 / #2 / canário #3 / resumos" "$(state)" "0/0/0/0"

step "5. sob supervisão, morto depois de enviar o primeiro comentário"
world sup
go supervise "$PROG" injected.json sup --inject-kills sent:1 --backoff 0.2 > "$OUT/sup.out" 2>&1 || { cat "$OUT/sup.out"; die "o supervisor falhou"; }
grep -E "supervisor:" "$OUT/sup.out" | sed 's/^/   /' | cut -c1-120
check "comentários em #1 / #2 / canário #3 / resumos" "$(state)" "1/1/0/1"

printf '\n\033[1;32mv1.0-RC7: todas as conferências passaram.\033[0m\n'

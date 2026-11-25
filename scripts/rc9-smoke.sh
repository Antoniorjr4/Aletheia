#!/usr/bin/env bash
# Aletheia v1.0-RC9 — ferramentas de um servidor MCP sob os tipos do programa, sem rede.
#
#   scripts/rc9-smoke.sh
#
# Sobre examples/rc9/triage.ath, contra a face `ath-world mcp`, que anuncia as ferramentas de
# issues com as definições exatas do servidor MCP oficial do GitHub e, com `--mode`, se comporta
# como um servidor hostil:
#   1. a linguagem: `bind … mcp`, conferido antes de rodar (E0435, E0439)
#   2. a triagem por MCP, nas duas eras do protocolo (2026-07-28 e o `initialize` legado)
#   3. descrições envenenadas: o prompt do modelo é o mesmo, byte a byte
#   4. deriva de esquema e anotação que admite perigo: recusadas antes de qualquer efeito
#   5. o servidor pede amostragem, elicitação e raízes: tudo recusado, o modelo é consultado 1 vez
#   6. sob supervisão, morto depois do primeiro comentário: termina, sem duplicar
# Requisitos: Rust (cargo) e python3. Nenhum modelo pago, nenhuma rede.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OUT="${RC9_SMOKE_DIR:-$ROOT/target/rc9-smoke}"
PROG=examples/rc9/triage.ath
export GH_TOKEN="tok-demo-rc9" GITHUB_OWNER=dono GITHUB_REPO=repo

step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
ok()   { printf '   \033[32m✓\033[0m %s\n' "$*"; }
die()  { printf '   \033[31m✗ %s\033[0m\n' "$*"; exit 1; }
check() { if [ "$2" = "$3" ]; then ok "$1: $2"; else die "$1: obtido \`$2\`, esperado \`$3\`"; fi; }

step "0. construção"
cargo build --release --quiet -p athc -p ath-world
ATHC="$ROOT/target/release/athc"; WORLD="$ROOT/target/release/ath-world"
rm -rf "${OUT:?}"; mkdir -p "$OUT"
world() { # world <nome> <modos>: um banco novo, com as três issues; o comando do servidor
  local db="$OUT/$1.sqlite"
  "$WORLD" init --db "$db" --seed examples/rc1/fixtures/seed.json > /dev/null
  python3 - "$db" <<'PY'
import sqlite3, sys
c = sqlite3.connect(sys.argv[1])
c.executemany("INSERT INTO gh_issues (title, body) VALUES (?, '')",
              [("bomba P-101 com vibração alta",), ("vazamento no selo da P-205",), ("canário: nenhuma nota aqui",)])
c.commit()
PY
  export GITHUB_MCP_SERVER="$WORLD mcp --db $db${2:+ --mode $2}"
  DB="$db"
}
state() { python3 -c "
import sqlite3, sys
c = sqlite3.connect(sys.argv[1])
n = lambda i: c.execute('SELECT COUNT(*) FROM gh_comments WHERE issue = ?', (i,)).fetchone()[0]
s = c.execute('SELECT COUNT(*) FROM gh_issues WHERE number > 3').fetchone()[0]
print(f'{n(1)}/{n(2)}/{n(3)}/{s}')" "$DB"; }
hashes() { python3 -c "
import json, sys
print(' '.join(json.loads(l)['detail']['prompt_hash'][:12] for l in open(sys.argv[1]) if '\"kind\":\"infer\"' in l))" "$1"; }
echo '{ "inventories": { "issues": ["1", "2"] } }' > "$OUT/trusted.json"
note() { echo "{ \"kind\": \"Note\", \"issue\": \"$1\", \"text\": \"$2\" }"; }
echo "{ \"planner\": [ { \"kind\": \"Plan\", \"notes\": [ $(note 1 'medir a vibração'), $(note 3 CANARIO), $(note 2 'trocar o selo') ] } ] }" > "$OUT/models.json"
go() {
  local sub="$1" prog="$2" id="$3"; shift 3
  "$ATHC" "$sub" "$prog" --provider scripted --models "$OUT/models.json" --trusted "$OUT/trusted.json" --timeout 5 \
    --run-id "$id" --wal "$OUT/$id.wal" --journal "$OUT/$id.jsonl" --arg '"1"' --arg '"2"' "$@"
}

step "1. a linguagem"
grep -nE "mcp  =|auth =|-> html_url|\.find" "$PROG" | sed 's/^/   | /' | cut -c1-150
"$ATHC" check "$PROG" | grep "ligação" | sed 's/^/   /'
sed 's/issue_number: int(issue), body: body) -> html_url/issue_number: int(issue)) -> html_url/' "$PROG" > "$OUT/m1.ath"
"$ATHC" check "$OUT/m1.ath" > "$OUT/m1.out" 2>&1 && die "aceito"; grep -q E0439 "$OUT/m1.out" && ok "um parâmetro que não chega ao servidor: E0439"

step "2. a triagem por MCP, nas duas eras do protocolo"
for era in "" legacy; do
  world "era$era" "$era"
  go run "$PROG" "era$era" > "$OUT/era$era.out" 2>&1 || { cat "$OUT/era$era.out"; die "a execução falhou"; }
  grep -oE "ligada\(s\) por MCP \(stdio\) a [^;]*" "$OUT/era$era.out" | sed 's/^/   /' 
  check "comentários em #1 / #2 / canário #3 / resumos" "$(state)" "1/1/0/1"
done

step "3. descrições e instruções envenenadas não chegam ao modelo"
world clean ""; go run "$PROG" clean > /dev/null 2>&1
world poison poison; go run "$PROG" poison > /dev/null 2>&1
check "hash do prompt, servidor limpo × envenenado" "$(hashes "$OUT/poison.jsonl")" "$(hashes "$OUT/clean.jsonl")"
check "comentários em #1 / #2 / canário #3 / resumos" "$(state)" "1/1/0/1"

step "4. o que o servidor anuncia é conferido antes de qualquer efeito"
for m in drift:R0646 lie-read:R0647; do
  world "${m%%:*}" "${m%%:*}"
  go run "$PROG" "${m%%:*}" > "$OUT/${m%%:*}.out" 2>&1 && die "aceito"
  grep -oE "R064[67][^;]*" "$OUT/${m%%:*}.out" | head -1 | sed 's/^/   /' | cut -c1-170
  grep -q "${m#*:}" "$OUT/${m%%:*}.out" || die "esperado ${m#*:}"
  check "nada saiu" "$(state)" "0/0/0/0"
done

step "5. o servidor não alcança o modelo nem o usuário"
world ask ask
go run "$PROG" ask > "$OUT/ask.out" 2>&1 || { cat "$OUT/ask.out"; die "a execução falhou"; }
grep -oE "[0-9]+ pedido\(s\) do servidor ao cliente recusado\(s\)[^;]*" "$OUT/ask.out" | sed 's/^/   /' 
check "consultas ao modelo" "$(hashes "$OUT/ask.jsonl" | wc -w | tr -d ' ')" "1"
check "comentários em #1 / #2 / canário #3 / resumos" "$(state)" "1/1/0/1"

step "6. sob supervisão, morto depois de enviar o primeiro comentário"
world sup ""
go supervise "$PROG" sup --inject-kills sent:1 --backoff 0.2 > "$OUT/sup.out" 2>&1 || { cat "$OUT/sup.out"; die "o supervisor falhou"; }
grep -E "supervisor:" "$OUT/sup.out" | sed 's/^/   /' | cut -c1-120
check "comentários em #1 / #2 / canário #3 / resumos" "$(state)" "1/1/0/1"

printf '\n\033[1;32mv1.0-RC9: todas as conferências passaram.\033[0m\n'

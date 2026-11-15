#!/usr/bin/env bash
# Aletheia v1.0-RC5 — o agente inteiro na linguagem, num comando.
#
#   scripts/rc5-smoke.sh
#
# Constrói o compilador e o serviço local e mostra, com processos, disco, HTTP e sinais de
# verdade, sobre examples/rc5/treasury.ath (a tesouraria da RC4 com as ferramentas ligadas por
# `bind` à API REST do banco):
#   1. a linguagem: a ligação declarada; E0431, E0432, E0433, E0434; segredo literal não é sintaxe
#   2. uma execução pela API HTTP, sem código de cola, com as garantias da RC4; o segredo não
#      está em arquivo algum; sem o segredo (R0642) ou sem a ligação (R0641), nada executa
#   3. o provedor que devolve um valor fora do tipo: a culpa é dele (R0640)
#   4. SIGKILL depois do segundo pagamento: `athc resume` termina o agente; cada operação
#      aplicada uma só vez; o modelo não é consultado de novo; o orçamento é o da execução sem queda
#   5. a retomada também é morta; a terceira termina; retomar a execução terminada não chama ninguém
#   6. a retomada recusa outra história (R0652) e uma execução encerrada por `recover` (R0653)
#   7. (RC5.1) a fita truncada é recusada pela marca da âncora (L0008); um literal IPv6 que não
#      é o próprio host não recebe o segredo em claro (R0643); 202 Accepted não é efeito concluído
#
# Três papéis, dois processos: o provedor (o banco: API REST com token, recibos assinados) e a
# âncora (também a testemunha de tempo), em bancos SQLite separados.
# Requisitos: Rust (cargo) e python3. Nenhum modelo pago: o modelo é um roteiro local.
# As chaves são os vetores de teste 1, 2 e 3 da RFC 8032; o token é de demonstração.
# Cada verificação que falha encerra o script com código 1.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OUT="${RC5_SMOKE_DIR:-$ROOT/target/rc5-smoke}"
PROG=examples/rc5/treasury.ath
FX=examples/rc1/fixtures
KEY=examples/rc2/fixtures/DEMO-rfc8032.key
WK=examples/rc4/fixtures
REQUEST='"Pague 120.00 a ACME (NF 4471) e 75.50 a BETA; agende 250.00 para BETA em 2026-12-01."'
export BANK_TOKEN="tok-DEMO-5f1c9a7e2b4d-NAO-E-UM-SEGREDO-REAL"

step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
ok()   { printf '   \033[32m✓\033[0m %s\n' "$*"; }
die()  { printf '   \033[31m✗ %s\033[0m\n' "$*"; exit 1; }
check() { if [ "$2" = "$3" ]; then ok "$1: $2"; else die "$1: obtido \`$2\`, esperado \`$3\`"; fi; }
blocked() {
  local what="$1" code="$2"; shift 2
  local before; before="$(effects)"
  "$@" > "$OUT/last.out" 2>&1 || true
  grep -q "$code" "$OUT/last.out" || { cat "$OUT/last.out"; die "$what: esperado $code"; }
  [ "$(effects)" = "$before" ] || die "$what: um efeito saiu"
  ok "$what: $(grep -o "$code[^\"]*" "$OUT/last.out" | head -1 | cut -c1-110)"
}

step "0. construção"
cargo build --release --quiet -p athc -p ath-world
ATHC="$ROOT/target/release/athc"
WORLD="$ROOT/target/release/ath-world"
rm -rf "${OUT:?}"; mkdir -p "$OUT"
ok "athc e ath-world construídos"

PIDS=()
trap 'for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null || true; done' EXIT
# world <nome>: um banco (API REST com token) e uma âncora novos
world() {
  for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null || true; wait "$p" 2>/dev/null || true; done
  PIDS=()
  DB="$OUT/$1.bank.sqlite"; ADB="$OUT/$1.anchor.sqlite"
  : > "$OUT/addr"; : > "$OUT/aaddr"
  "$WORLD" init --db "$DB" --seed "$FX/seed.json" > /dev/null
  "$WORLD" serve --db "$DB" --port-file "$OUT/addr" --sign-key "$WK/DEMO-bank.key" --http --token-env BANK_TOKEN > "$OUT/$1.bank.out" 2>&1 & PIDS+=($!)
  "$WORLD" serve --db "$ADB" --port-file "$OUT/aaddr" --sign-key "$WK/DEMO-notary.key" > "$OUT/$1.anchor.out" 2>&1 & PIDS+=($!)
  for _ in $(seq 1 100); do [ -s "$OUT/addr" ] && [ -s "$OUT/aaddr" ] && break; sleep 0.05; done
  [ -s "$OUT/addr" ] && [ -s "$OUT/aaddr" ] || die "os serviços não subiram"
  export BANK_URL="http://$(cat "$OUT/addr")"; ANCHOR="tcp://$(cat "$OUT/aaddr")"
}
get() { "$WORLD" state --db "$DB" --get "$1"; }
lc() { "$WORLD" state --db "$ADB" --get "lifecycle.$1" | sed 's/null/0/'; }
effects() { echo "$(get counts.pays_posted)/$(get counts.scheduled)/$(get counts.receipts)"; }
calls() { python3 -c "
import sqlite3,sys
print(sqlite3.connect(sys.argv[1]).execute(\"select count(*) from log where line like '%\\\"op\\\":\\\"call\\\"%' or line like 'call %'\").fetchone()[0])" "$DB"; }
authorize() {
  local id="$1"
  echo '{"grants": []}' > "$OUT/$id.grants.json"
  "$ATHC" grant --run "$id" --origin "confirmation:rc5-smoke" --out "$OUT/$id.grants.json" --from "$FX/intent.json" \
    --sign "$KEY" --program "$PROG" --ttl 600 > /dev/null
  cp "$FX/trusted.json" "$OUT/$id.trusted.json"
  "$ATHC" sign --key "$KEY" --trusted "$OUT/$id.trusted.json" --run "$id" --program "$PROG" --ttl 600 > /dev/null
  "$ATHC" sign --key "$KEY" --run "$id" --arg "$REQUEST" --program "$PROG" --ttl 600 --out "$OUT/$id.args.sig"
}
# go <run|resume> <execução> <wal> [opções]: sem --host; o provedor é o do `bind`
go() {
  local sub="$1" id="$2" wal="$3"; shift 3
  local idflag=(); [ "$sub" = run ] && idflag=(--run-id "$id")
  "$ATHC" "$sub" "$PROG" --provider scripted --models "$FX/${MODELS:-models-legit.json}" \
    --trusted "$OUT/$id.trusted.json" --timeout 5 --anchor "$ANCHOR" --clock "$ANCHOR" \
    "${idflag[@]}" --grants "$OUT/$id.grants.json" --wal "$OUT/$wal.wal" \
    --journal "$OUT/$wal.$sub.jsonl" --report "$OUT/$wal.$sub.report.json" \
    --args-sig "$OUT/$id.args.sig" --arg "$REQUEST" "$@"
}
spent() { python3 -c "
import json,sys; r=json.load(open(sys.argv[1])); print(r['spent']['tok'], r['spent']['usd'])" "$1"; }
no_secret() { if grep -rqa --exclude='*.sqlite*' -- "$BANK_TOKEN" "$OUT"; then die "$1: o segredo está em disco"; else ok "$1: o segredo não está em arquivo algum de $OUT"; fi; }

step "1. a linguagem: a ligação"
sed -n '/^bind/,/^}/p' "$PROG" | sed 's/^/   | /'
"$ATHC" check "$PROG" | grep "ligação"
world lang
mut() { sed "$1" "$PROG" > "$OUT/$2.ath"; }
mut 's/  wire      = "POST/  wirre     = "POST/' m1; blocked "rota para o que não é ferramenta" E0431 "$ATHC" check "$OUT/m1.ath"
mut 's|env("BANK_URL")|"http://bank.example"|' m3;  blocked "segredo em claro para outro host" E0433 "$ATHC" check "$OUT/m3.ath"
mut 's|"POST /v1/payments -> id"|"GET /v1/payments -> id"|' m4; blocked "operação com chave num GET" E0434 "$ATHC" check "$OUT/m4.ath"
mut 's|secret("BANK_TOKEN")|"tok-123"|' m5; blocked "segredo literal" "nunca um literal do programa" "$ATHC" check "$OUT/m5.ath"

step "2. uma execução pela API HTTP, sem código de cola"
world run; authorize r
go run r r > "$OUT/r.out" 2>&1 || { cat "$OUT/r.out"; die "a execução falhou"; }
grep -E "ligada|resultado" "$OUT/r.out" | sed 's/^/   /' | cut -c1-150
check "pagamentos/agendamentos/comprovantes" "$(effects)" "2/1/1"
check "ciclo de vida ancorado (send/outcome)" "$(lc send)/$(lc outcome)" "3/3"
no_secret "execução"
cp "$OUT/r.run.report.json" "$OUT/reference.report.json"
authorize r2
blocked "sem o segredo no ambiente" R0642 env -u BANK_TOKEN "$ATHC" run "$PROG" --provider scripted --models "$FX/models-legit.json" \
  --trusted "$OUT/r2.trusted.json" --anchor "$ANCHOR" --clock "$ANCHOR" --run-id r2 --grants "$OUT/r2.grants.json" \
  --wal "$OUT/r2a.wal" --journal "$OUT/r2a.jsonl" --args-sig "$OUT/r2.args.sig" --arg "$REQUEST"
blocked "sem a ligação" R0641 go run r2 r2b --no-bind

step "3. o provedor que devolve um valor fora do tipo"
cat > "$OUT/probe.ath" <<'ATH'
module rc5.probe

tool balance(c: Cap[Bank.Read], account: Text[..16]) -> Text[..4] ^ Untrusted
  ! { Bank.Read } reversible latency 5.0

bind api {
  url     = env("BANK_URL")
  auth    = secret("BANK_TOKEN")
  balance = "GET /v1/balance -> id"
}

agent Probe(bank: Cap[Bank.Read])
  budget [tok <= 10, usd <= 0.01, wall <= 30.0]
{
  on ask(account: Text[..16] ^ Trusted) -> Text ! { Bank.Read } {
    let b = balance(bank, account)
    "saldo lido"
  }
}
ATH
"$ATHC" run "$OUT/probe.ath" --arg '"TREASURY"' --journal "$OUT/probe.jsonl" > "$OUT/probe.out" 2>&1 && die "o valor fora do tipo entrou no programa"
grep -q R0640 "$OUT/probe.out" || { cat "$OUT/probe.out"; die "esperado R0640"; }
ok "o saldo (7 caracteres) não cabe em Text[..4]: $(grep -o 'R0640[^;]*' "$OUT/probe.out" | head -1 | cut -c1-120)"

step "4. SIGKILL depois do segundo pagamento; athc resume termina o agente"
world resume; authorize k
set +e; go run k k --kill-at sent:2 > "$OUT/k.out" 2>&1; code=$?; set -e
check "o processo foi morto (SIGKILL)" "$code" "137"
check "antes da retomada" "$(effects)" "2/0/0"
# o modelo da retomada responderia outra coisa (uma remessa): ele não é consultado
MODELS=models-wire.json go resume k k > "$OUT/k.resume.out" 2>&1 || { cat "$OUT/k.resume.out"; die "a retomada falhou"; }
grep -E "retomada:|resultado" "$OUT/k.resume.out" | sed 's/^/   /' | cut -c1-150
check "depois da retomada" "$(effects)" "2/1/1"
check "remessas (o modelo não foi consultado de novo)" "$(get counts.wires)" "0"
check "orçamento gasto = o da execução sem queda (tok, US\$)" "$(spent "$OUT/k.resume.report.json")" "$(spent "$OUT/reference.report.json")"
no_secret "retomada"

step "5. a retomada também é morta; a terceira termina"
world again; authorize a
set +e; go run a a --kill-at sent:1 > /dev/null 2>&1; c1=$?
go resume a a --kill-at intent:1 > /dev/null 2>&1; c2=$?
go resume a a --kill-at confirmed:1 > /dev/null 2>&1; c3=$?; set -e
check "três quedas (SIGKILL)" "$c1/$c2/$c3" "137/137/137"
go resume a a > "$OUT/a.out" 2>&1 || { cat "$OUT/a.out"; die "a retomada falhou"; }
check "estado final" "$(effects)" "2/1/1"
before="$(calls)"
go resume a a > "$OUT/a2.out" 2>&1 || { cat "$OUT/a2.out"; die "a retomada da execução terminada falhou"; }
grep -q "0 chamada(s) ao vivo" "$OUT/a2.out" || { cat "$OUT/a2.out"; die "a retomada da execução terminada chamou alguém"; }
check "retomar a execução terminada: chamadas ao provedor" "$(calls)" "$before"

step "6. a retomada recusa outra história"
world refuse; authorize z
set +e; go run z z --kill-at sent:2 > /dev/null 2>&1; set -e
blocked "outros argumentos" R0652 "$ATHC" resume "$PROG" --provider scripted --models "$FX/models-legit.json" \
  --trusted "$OUT/z.trusted.json" --anchor "$ANCHOR" --clock "$ANCHOR" --grants "$OUT/z.grants.json" \
  --wal "$OUT/z.wal" --journal "$OUT/z.x.jsonl" --arg '"Pague 9000.00 a MALLORY."'
"$ATHC" recover "$PROG" --wal "$OUT/z.wal" --timeout 5 > "$OUT/z.recover.out" 2>&1 || true
check "athc recover encerrou a execução (saga compensada)" "$(get counts.pays_refunded)" "2"
blocked "execução encerrada por recover" R0653 go resume z z

step "7. RC5.1: a fita truncada, o host que não é o próprio, e o 202 Accepted"
world tape; authorize t
set +e; go run t t --kill-at sent:3 > /dev/null 2>&1; set -e
head -n 2 "$OUT/t.wal.tape" > "$OUT/t.tape.cut" && cp "$OUT/t.tape.cut" "$OUT/t.wal.tape"
blocked "a fita truncada antes da marca da âncora" L0008 go resume t t
blocked "segredo em claro para um literal IPv6 que não é o próprio host" R0643 \
  env BANK_URL='http://[::ffff:198.51.100.10]' "$ATHC" run "$OUT/probe.ath" --arg '"TREASURY"' --journal "$OUT/p6.jsonl"
blocked "url com credenciais" R0642 \
  env BANK_URL='http://localhost:x@evil.example' "$ATHC" run "$OUT/probe.ath" --arg '"TREASURY"' --journal "$OUT/p7.jsonl"
cat > "$OUT/note.ath" <<'ATH'
module rc5.note

tool note(c: Cap[Bank.Pay], text: Text[..16], k: own IdemKey) -> Text[..8]
  ! { Bank.Pay } irrevocable provider idempotent latency 5.0

bind api {
  url  = env("BANK_URL")
  auth = secret("BANK_TOKEN")
  note = "POST /v1/accepted -> id"
}

agent Clerk(bank: Cap[Bank.Pay], keys: Cap[IdemKey.Mint])
  budget [tok <= 10, usd <= 0.01, wall <= 30.0]
{
  on ask(text: Text[..16] ^ Trusted) -> Text ! { Bank.Pay, TimeoutUnknown } {
    commit note(bank, text, keys.mint())
  }

  handle timeout_unknown(p: own Pending[_]) -> Text {
    reconcile p fuel 2 else {
      transfer p
      "pendente"
    }
  }
}
ATH
"$ATHC" run "$OUT/note.ath" --arg '"oi"' --wal "$OUT/note.wal" --journal "$OUT/note.jsonl" > "$OUT/note.out" 2>&1 || true
grep -q 'resultado: "pendente"' "$OUT/note.out" || { cat "$OUT/note.out"; die "202 Accepted foi tratado como efeito concluído"; }
grep -q '"status":"done"' "$OUT/note.wal" && die "202 Accepted fechou a operação como done"
ok "202 Accepted: resultado desconhecido, a obrigação foi transferida (não fechada como done)"

printf '\n\033[1;32mv1.0-RC5: todas as conferências passaram.\033[0m\n'

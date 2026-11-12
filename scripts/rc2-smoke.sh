#!/usr/bin/env bash
# Aletheia v1.0-RC2 — a fronteira entre autorização, persistência e efeito, num comando.
#
#   scripts/rc2-smoke.sh
#
# Constrói o compilador e o serviço local, e mostra, com processos, disco e sinais de verdade,
# sobre examples/rc2/treasury.ath (que declara a autoridade que pode conceder):
#   1. o programa verificado, com a autoridade no Codex
#   2. uma execução com tudo assinado pela autoridade
#   3. a janela: SIGKILL entre a autorização e a persistência; nenhuma concessão se perde
#   4. a corrida: quatro processos independentes e as mesmas concessões; um efeito por concessão
#   5. um executor por WAL
#   6. falsificações: concessões, fotografia confiável e pedido sem a assinatura da autoridade
#   7. o registro de consumo ausente interrompe a recuperação
#
# Requisitos: Rust (cargo) e um compilador C. Nenhum modelo pago: o modelo é um roteiro local.
# A chave privada usada é o vetor de teste 1 da RFC 8032 (pública, de demonstração).
# Cada verificação que falha encerra o script com código 1.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OUT="${RC2_SMOKE_DIR:-$ROOT/target/rc2-smoke}"
PROG=examples/rc2/treasury.ath
FX=examples/rc1/fixtures
KEY=examples/rc2/fixtures/DEMO-rfc8032.key
REQUEST='"Pague 120.00 a ACME (NF 4471) e 75.50 a BETA; agende 250.00 para BETA em 2026-12-01."'

step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
ok()   { printf '   \033[32m✓\033[0m %s\n' "$*"; }
die()  { printf '   \033[31m✗ %s\033[0m\n' "$*"; exit 1; }
check() { if [ "$2" = "$3" ]; then ok "$1: $2"; else die "$1: obtido \`$2\`, esperado \`$3\`"; fi; }
# expect_fail "descrição" código-de-diagnóstico comando…: o comando falha e cita o código
expect_fail() {
  local what="$1" code="$2"; shift 2
  if "$@" > "$OUT/last.out" 2>&1; then cat "$OUT/last.out"; die "$what: deveria ter sido recusado"; fi
  grep -q "$code" "$OUT/last.out" || { cat "$OUT/last.out"; die "$what: esperado $code"; }
  ok "$what: recusado ($(grep -o "$code[^;]*" "$OUT/last.out" | head -1 | cut -c1-110))"
}

step "0. construção"
cargo build --release --quiet -p athc -p ath-world
ATHC="$ROOT/target/release/athc"
WORLD="$ROOT/target/release/ath-world"
rm -rf "$OUT"; mkdir -p "$OUT"
ok "athc e ath-world construídos"

WORLD_PID=""
trap '[ -n "$WORLD_PID" ] && kill "$WORLD_PID" 2>/dev/null || true' EXIT
# world <nome>: um serviço novo, com o estado inicial, para cada demonstração
world() {
  [ -n "$WORLD_PID" ] && { kill "$WORLD_PID" 2>/dev/null || true; wait "$WORLD_PID" 2>/dev/null || true; }
  DB="$OUT/$1.sqlite"; rm -f "$OUT/addr"
  "$WORLD" init --db "$DB" --seed "$FX/seed.json" > /dev/null
  "$WORLD" serve --db "$DB" --port-file "$OUT/addr" > "$OUT/$1.world.out" 2>&1 &
  WORLD_PID=$!
  for _ in $(seq 1 100); do [ -s "$OUT/addr" ] && break; sleep 0.05; done
  [ -s "$OUT/addr" ] || die "o serviço não subiu"
  ADDR="$(cat "$OUT/addr")"
}
get() { "$WORLD" state --db "$DB" --get "$1"; }
spent() { if [ -f "$OUT/$1.grants.json.spent" ]; then grep -o '"grants":\["[0-9a-f]*"' "$OUT/$1.grants.json.spent" | sort -u | wc -l | tr -d ' '; else echo 0; fi; }

# a autoridade assina as três fontes de uma execução: concessões, fotografia e pedido
authorize() {
  local id="$1"
  "$ATHC" grant --run "$id" --origin "confirmation:rc2-smoke" --out "$OUT/$id.grants.json" --from "$FX/intent.json" --sign "$KEY" > /dev/null
  cp "$FX/trusted.json" "$OUT/$id.trusted.json"
  "$ATHC" sign --key "$KEY" --trusted "$OUT/$id.trusted.json" > /dev/null
  "$ATHC" sign --key "$KEY" --run "$id" --arg "$REQUEST" --out "$OUT/$id.args.sig"
}
# run <execução> <wal> [opções]: dois processos da mesma execução usam WALs diferentes
run() {
  local id="$1" wal="$2"; shift 2
  "$ATHC" run "$PROG" --provider scripted --models "$FX/models-legit.json" \
    --host "tcp://$ADDR" --trusted "$OUT/$id.trusted.json" --timeout 5 \
    --run-id "$id" --grants "$OUT/$id.grants.json" --wal "$OUT/$wal.wal" \
    --journal "$OUT/$wal.journal.jsonl" --report "$OUT/$wal.report.json" \
    --args-sig "$OUT/$id.args.sig" --arg "$REQUEST" "$@"
}
recover() { local wal="$1"; shift; "$ATHC" recover "$PROG" --wal "$OUT/$wal.wal" --host "tcp://$ADDR" --timeout 5 "$@"; }

step "1. o programa: a autoridade faz parte dele"
"$ATHC" check "$PROG" | tail -1
"$ATHC" hash "$PROG" | grep authority
ok "a chave pública da autoridade entra no hash do programa (Codex)"

step "2. execução com tudo assinado pela autoridade"
world legit; authorize ok
run ok ok
check "saldo da tesouraria" "$(get accounts.TREASURY)" "9804.50"
check "agendamentos" "$(get counts.scheduled)" "1"
check "concessões consumidas" "$(spent ok)" "3"
check "fontes atestadas no journal" "$(grep -c '"kind":"attested"' "$OUT/ok.journal.jsonl")" "2"

step "3. a janela: SIGKILL entre a autorização e a persistência"
for point in authorized logged; do
  world "window-$point"; authorize "w-$point"
  set +e; run "w-$point" "w-$point" --kill-at "$point:1" > "$OUT/w-$point.out" 2>&1; rc=$?; set -e
  check "$point:1 · o processo morreu por SIGKILL (137)" "$rc" "137"
  check "$point:1 · concessões consumidas depois da queda" "$(spent "w-$point")" "0"
  check "$point:1 · requisições que chegaram ao serviço para a operação" "$("$WORLD" log --db "$DB" | grep -c "w-$point:k1" || true)" "0"
  recover "w-$point" | sed 's/^/   | /'
  check "$point:1 · concessões consumidas depois da recuperação" "$(spent "w-$point")" "0"
  # a autorização não se perdeu: uma nova execução faz o que foi confirmado, uma vez
  run "w-$point" "w-$point-again" > "$OUT/w-$point-again.out"
  check "$point:1 · nova execução com as mesmas concessões: tesouraria" "$(get accounts.TREASURY)" "9804.50"
  check "$point:1 · pagamentos / agendamentos / comprovantes" "$(get counts.pays_posted)/$(get counts.scheduled)/$(get counts.receipts)" "2/1/1"
done
grep -q 'recovery:unspent' "$OUT/w-logged.wal" || die "a intenção sem consumo deveria ter sido fechada como não enviada"
ok "logged:1 · a intenção sem consumo foi fechada no WAL como não enviada (recovery:unspent)"

step "4. a corrida: quatro processos independentes, as mesmas concessões"
world race; authorize race
for i in 1 2 3 4; do "$WORLD" fault --db "$DB" --tool statement --kind hang --ms 700 > /dev/null; done
pids=()
for i in 1 2 3 4; do (run race "race-p$i" > "$OUT/race-p$i.out" 2>&1 || true) & pids+=($!); done
for p in "${pids[@]}"; do wait "$p"; done
check "concessões consumidas (cada uma, uma vez)" "$(spent race)" "3"
check "linhas no registro de consumo" "$(wc -l < "$OUT/race.grants.json.spent" | tr -d ' ')" "3"
check "agendamentos no serviço (provedor que não deduplica)" "$(get counts.scheduled)" "1"
check "saldo da tesouraria (uma execução, não quatro)" "$(get accounts.TREASURY)" "9804.50"
check "processos recusados por R0632" "$(grep -l R0632 "$OUT"/race-p*.out | wc -l | tr -d ' ')" "3"

step "5. um executor por WAL"
world lock; authorize lock
"$WORLD" fault --db "$DB" --tool statement --kind hang --ms 2500 > /dev/null
run lock lock > "$OUT/lock.out" 2>&1 &
LIVE=$!
for _ in $(seq 1 200); do [ -s "$OUT/lock.wal" ] && break; sleep 0.01; done
expect_fail "segundo athc run no mesmo WAL" W0006 run lock lock
expect_fail "athc recover com a execução viva" W0006 recover lock
wait "$LIVE" || die "a execução viva deveria terminar bem"
check "a execução viva terminou · agendamentos" "$(get counts.scheduled)" "1"

step "6. falsificações: sem a chave privada da autoridade, ninguém concede"
world forge; authorize f
before="$(get accounts)$(get counts)"
"$ATHC" keygen --out "$OUT/outra.key" > "$OUT/outra.pub"
ok "uma segunda chave, válida, que não é a autoridade: $(cut -c1-24 "$OUT/outra.pub")…"
cp "$OUT/f.grants.json" "$OUT/f.grants.authentic"
# concessões sem assinatura
rm "$OUT/f.grants.json"; "$ATHC" grant --run f --origin confirmation:forjada --out "$OUT/f.grants.json" --from "$FX/intent.json" > /dev/null
expect_fail "concessões sem assinatura" G0001 run f f1
# concessões assinadas por outra chave
rm "$OUT/f.grants.json"; "$ATHC" grant --run f --origin confirmation:forjada --out "$OUT/f.grants.json" --from "$FX/intent.json" --sign "$OUT/outra.key" > /dev/null
expect_fail "concessões assinadas por outra chave" G0001 run f f2
cp "$OUT/f.grants.authentic" "$OUT/f.grants.json"
# a fotografia confiável com um beneficiário a mais, e a assinatura antiga
echo '{ "inventories": { "payees": ["ACME", "BETA", "MALLORY"] } }' > "$OUT/f.trusted.json"
expect_fail "fotografia confiável adulterada" R0635 run f f3
cp "$FX/trusted.json" "$OUT/f.trusted.json"
# outro pedido, com a assinatura do pedido original
expect_fail "pedido diferente do assinado" R0636 "$ATHC" run "$PROG" --provider scripted --models "$FX/models-legit.json" \
  --host "tcp://$ADDR" --trusted "$OUT/f.trusted.json" --run-id f --grants "$OUT/f.grants.json" --wal "$OUT/f4.wal" \
  --args-sig "$OUT/f.args.sig" --arg '"Pague 9120.00 a ACME."'
check "estado do serviço depois das quatro tentativas" "$(get accounts)$(get counts)" "$before"
check "concessões consumidas" "$(spent f)" "0"

step "7. sem o registro de consumo, a recuperação não adivinha"
world ledger; authorize l
set +e; run l l --kill-at intent:1 > "$OUT/l.out" 2>&1; set -e
mv "$OUT/l.grants.json.spent" "$OUT/l.spent.saved"
expect_fail "recuperação sem o registro de consumo" W0007 recover l
mv "$OUT/l.spent.saved" "$OUT/l.grants.json.spent"
recover l | sed 's/^/   | /'
check "com o registro de volta · a saga interrompida foi estornada" "$(get accounts.TREASURY)" "10000.00"

step "fim"
ok "v1.0-RC2: a fronteira entre autorização, persistência e efeito, demonstrada"
echo "   evidências em $OUT"

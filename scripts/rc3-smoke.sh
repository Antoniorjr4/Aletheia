#!/usr/bin/env bash
# Aletheia v1.0-RC3 — autoridade durável e integridade temporal, num comando.
#
#   scripts/rc3-smoke.sh
#
# Constrói o compilador e o serviço local e mostra, com processos, disco e sinais de verdade,
# sobre examples/rc3/treasury.ath (autoridade com escopo, validade e registro ancorado):
#   1. o programa: o escopo da autoridade fecha com os contratos (E0427)
#   2. uma execução legítima e a sua história: concessão → intenção → consumo → âncora → desfecho
#   3. retrocesso do registro de consumo: recusado (L0002); as concessões não voltam
#   4. recuperação com o registro retrocedido: recusada; com o registro verdadeiro, resolve
#   5. registro adulterado no meio: recusado (L0001)
#   6. SIGKILL entre o consumo e a âncora: a recuperação adianta a âncora
#   7. sem âncora não se consome (R0637, L0005)
#   8. frescor e escopo: fotografia expirada, de outra execução e no formato do RC2; concessão
#      de outro programa
#
# Dois serviços: o provedor (o banco) e a âncora, em processos e bancos separados.
# Requisitos: Rust (cargo) e um compilador C. Nenhum modelo pago: o modelo é um roteiro local.
# A chave privada usada é o vetor de teste 1 da RFC 8032 (pública, de demonstração).
# Cada verificação que falha encerra o script com código 1.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OUT="${RC3_SMOKE_DIR:-$ROOT/target/rc3-smoke}"
PROG=examples/rc3/treasury.ath
FX=examples/rc1/fixtures
KEY=examples/rc2/fixtures/DEMO-rfc8032.key
REQUEST='"Pague 120.00 a ACME (NF 4471) e 75.50 a BETA; agende 250.00 para BETA em 2026-12-01."'

step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
ok()   { printf '   \033[32m✓\033[0m %s\n' "$*"; }
die()  { printf '   \033[31m✗ %s\033[0m\n' "$*"; exit 1; }
check() { if [ "$2" = "$3" ]; then ok "$1: $2"; else die "$1: obtido \`$2\`, esperado \`$3\`"; fi; }
# expect_fail "descrição" código comando…: o comando falha e cita o código
expect_fail() {
  local what="$1" code="$2"; shift 2
  if "$@" > "$OUT/last.out" 2>&1; then cat "$OUT/last.out"; die "$what: deveria ter sido recusado"; fi
  grep -q "$code" "$OUT/last.out" || { cat "$OUT/last.out"; die "$what: esperado $code"; }
  ok "$what: recusado ($(grep -o "$code.*" "$OUT/last.out" | head -1 | cut -c1-118))"
}

step "0. construção"
cargo build --release --quiet -p athc -p ath-world
ATHC="$ROOT/target/release/athc"
WORLD="$ROOT/target/release/ath-world"
rm -rf "${OUT:?}"; mkdir -p "$OUT"
ok "athc e ath-world construídos"

PIDS=()
trap 'for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null || true; done' EXIT
# world <nome>: um provedor e uma âncora novos, em processos e bancos separados
world() {
  for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null || true; wait "$p" 2>/dev/null || true; done
  PIDS=()
  DB="$OUT/$1.bank.sqlite"; ADB="$OUT/$1.anchor.sqlite"
  : > "$OUT/addr"; : > "$OUT/aaddr"
  "$WORLD" init --db "$DB" --seed "$FX/seed.json" > /dev/null
  "$WORLD" serve --db "$DB" --port-file "$OUT/addr" > "$OUT/$1.bank.out" 2>&1 & PIDS+=($!)
  "$WORLD" serve --db "$ADB" --port-file "$OUT/aaddr" > "$OUT/$1.anchor.out" 2>&1 & PIDS+=($!)
  for _ in $(seq 1 100); do [ -s "$OUT/addr" ] && [ -s "$OUT/aaddr" ] && break; sleep 0.05; done
  [ -s "$OUT/addr" ] && [ -s "$OUT/aaddr" ] || die "os serviços não subiram"
  ADDR="$(cat "$OUT/addr")"; ANCHOR="tcp://$(cat "$OUT/aaddr")"
}
get() { "$WORLD" state --db "$DB" --get "$1"; }
anchored() { "$WORLD" state --db "$ADB" --get anchored; }
ledger() { echo "$OUT/$1.grants.json.spent"; }
spent() { if [ -f "$(ledger "$1")" ]; then wc -l < "$(ledger "$1")" | tr -d ' '; else echo 0; fi; }
effects() { echo "$(get counts.pays_posted)/$(get counts.scheduled)/$(get counts.receipts)"; }
NOW="$(date +%s)"

# grants <execução> <programa>: as concessões, assinadas PARA o programa, PARA a execução, POR 600 s
grants() {
  local id="$1" prog="$2"
  : > "$OUT/$id.grants.json"; echo '{"grants": []}' > "$OUT/$id.grants.json"
  "$ATHC" grant --run "$id" --origin "confirmation:rc3-smoke" --out "$OUT/$id.grants.json" --from "$FX/intent.json" \
    --sign "$KEY" --program "$prog" --ttl 600 > /dev/null
}
# a autoridade assina as três fontes: concessões, fotografia e pedido
authorize() {
  local id="$1"
  grants "$id" "$PROG"
  cp "$FX/trusted.json" "$OUT/$id.trusted.json"
  "$ATHC" sign --key "$KEY" --trusted "$OUT/$id.trusted.json" --run "$id" --program "$PROG" --ttl 600 > /dev/null
  "$ATHC" sign --key "$KEY" --run "$id" --arg "$REQUEST" --program "$PROG" --ttl 600 --out "$OUT/$id.args.sig"
}
# run <execução> <wal> [opções]
run() {
  local id="$1" wal="$2"; shift 2
  "$ATHC" run "$PROG" --provider scripted --models "$FX/models-legit.json" \
    --host "tcp://$ADDR" --trusted "$OUT/$id.trusted.json" --timeout 5 --anchor "${ANCHOR_URL:-$ANCHOR}" \
    --run-id "$id" --grants "$OUT/$id.grants.json" --wal "$OUT/$wal.wal" \
    --journal "$OUT/$wal.journal.jsonl" --args-sig "$OUT/$id.args.sig" --arg "$REQUEST" "$@"
}
down() { ANCHOR_URL="tcp://127.0.0.1:1" run "$@"; }
recover() { local wal="$1"; shift; "$ATHC" recover "$PROG" --wal "$OUT/$wal.wal" --host "tcp://$ADDR" --timeout 5 "$@"; }

step "1. o programa: identidade, contexto, validade e história da autoridade"
"$ATHC" check "$PROG" | tail -1
sed -n '/^authority/,/^}/p' "$PROG" | sed 's/^/   | /'
sed 's/ops = "pay, schedule, wire"/ops = "pay, schedule"/' "$PROG" > "$OUT/narrow.ath"
expect_fail "escopo que não cobre o contrato de wire" E0427 "$ATHC" check "$OUT/narrow.ath"

step "2. execução legítima, e a história de cada efeito"
world legit; authorize ok
run ok ok | tail -4
check "pagamentos / agendamentos / comprovantes" "$(effects)" "2/1/1"
check "consumos no registro" "$(spent ok)" "3"
check "marca da âncora (em outro processo, outro banco)" "$(anchored)" "3"
check "desfechos do WAL que citam o seu consumo" "$(grep '"kind":"close"' "$OUT/ok.wal" | grep -c '"spend":"')" "3"
H="$(tail -1 "$(ledger ok)" | grep -o '"hash":"[0-9a-f]*"' | cut -d'"' -f4)"
grep '"kind":"close"' "$OUT/ok.wal" | grep -q "\"spend\":\"$H\"" || die "o desfecho do agendamento deveria citar o último consumo"
ok "o desfecho do agendamento cita o consumo #${H:0:12}, que cita a intenção, que cita a concessão assinada"

step "3. retrocesso do registro de consumo"
cp "$(ledger ok)" "$OUT/ok.ledger.real"
head -n 2 "$OUT/ok.ledger.real" > "$(ledger ok)"
expect_fail "última linha apagada" L0002 run ok back1
: > "$(ledger ok)"
expect_fail "registro esvaziado" L0002 run ok back2
check "nenhum efeito repetido" "$(effects)" "2/1/1"
cp "$OUT/ok.ledger.real" "$(ledger ok)"
run ok back3 > "$OUT/back3.out" 2>&1 || true
grep -q "nenhuma operação executada" "$OUT/back3.out" || die "com o registro verdadeiro, as concessões deveriam estar gastas"
check "com o registro verdadeiro de volta: concessões gastas, nenhum efeito novo" "$(effects)" "2/1/1"

step "4. a recuperação não reintegra uma concessão cujo efeito saiu"
world recover; authorize k
set +e; run k k --kill-at sent:3 > "$OUT/k.out" 2>&1; rc=$?; set -e
check "SIGKILL depois do envio do agendamento (137)" "$rc" "137"
check "o agendamento aconteceu no serviço" "$(get counts.scheduled)" "1"
cp "$(ledger k)" "$OUT/k.ledger.real"; head -n 2 "$OUT/k.ledger.real" > "$(ledger k)"
cp "$OUT/k.wal" "$OUT/k.wal.before"
expect_fail "recuperação com o consumo apagado" L0002 recover k
cmp -s "$OUT/k.wal" "$OUT/k.wal.before" || die "a recuperação recusada não deveria escrever no WAL"
ok "a recuperação recusada não escreveu no WAL nem concluiu que a operação \"não saiu\""
cp "$OUT/k.ledger.real" "$(ledger k)"
recover k | grep -E 'k3|nada pendente' | sed 's/^/   | /'
check "com o registro verdadeiro: resolvido por consulta, um agendamento" "$(get counts.scheduled)" "1"

step "5. registro adulterado no meio"
world tamper; authorize t
run t t > /dev/null
cp "$(ledger t)" "$OUT/t.ledger.real"
sed '2s/"tool":"pay"/"tool":"PAY"/' "$OUT/t.ledger.real" > "$(ledger t)"
expect_fail "uma linha do meio alterada" L0001 run t t2
sed '2d' "$OUT/t.ledger.real" > "$(ledger t)"
expect_fail "uma linha do meio removida" L0001 run t t3
check "nenhum efeito repetido" "$(effects)" "2/1/1"

step "6. SIGKILL entre o consumo e a âncora"
world spent; authorize s
set +e; run s s --kill-at spent:1 > "$OUT/s.out" 2>&1; rc=$?; set -e
check "o processo morreu por SIGKILL (137)" "$rc" "137"
check "consumos no registro / marca da âncora" "$(spent s)/$(anchored)" "1/0"
check "requisições do pagamento que chegaram ao serviço" "$("$WORLD" log --db "$DB" | grep -c 's:k1' || true)" "0"
recover s | grep -E 'k1|saga|nada pendente' | sed 's/^/   | /'
check "depois da recuperação: registro / âncora na mesma marca" "$(spent s)/$(anchored)" "1/1"

step "7. sem a âncora não se consome"
world noanchor; authorize n
before="$(get accounts)$(get counts)"
expect_fail "âncora fora do ar" L0005 down n n1
expect_fail "sem --anchor" R0637 "$ATHC" run "$PROG" --provider scripted --models "$FX/models-legit.json" \
  --host "tcp://$ADDR" --trusted "$OUT/n.trusted.json" --run-id n --grants "$OUT/n.grants.json" --wal "$OUT/n2.wal" \
  --args-sig "$OUT/n.args.sig" --arg "$REQUEST"
check "estado do serviço" "$(get accounts)$(get counts)" "$before"

step "8. frescor e escopo das fontes de autoridade"
authorize ontem; authorize f
"$ATHC" sign --key "$KEY" --trusted "$OUT/f.trusted.json" --run f --program "$PROG" --issued "$((NOW - 7200))" --ttl 600 > /dev/null
expect_fail "fotografia confiável expirada" R0635 run f f1
cp "$OUT/ontem.trusted.json.sig" "$OUT/f.trusted.json.sig"
expect_fail "fotografia autêntica de outra execução" R0635 run f f2
"$ATHC" sign --key "$KEY" --trusted "$OUT/f.trusted.json" > /dev/null
expect_fail "fotografia assinada no formato do RC2 (sem carimbo)" R0635 run f f3
"$ATHC" sign --key "$KEY" --trusted "$OUT/f.trusted.json" --run f --program "$PROG" --ttl 86400 > /dev/null
expect_fail "prazo maior que o max_age da autoridade" R0635 run f f4
"$ATHC" sign --key "$KEY" --trusted "$OUT/f.trusted.json" --run f --program "$PROG" --ttl 600 > /dev/null
sed 's/usd <= 0.10/usd <= 0.11/' "$PROG" > "$OUT/other.ath"
grants f "$OUT/other.ath"
expect_fail "concessões assinadas para outro programa" G0001 run f f5
check "estado do serviço depois das cinco tentativas" "$(get accounts)$(get counts)" "$before"
authorize f
run f f6 > /dev/null
check "com as fontes frescas e deste programa: executa" "$(effects)" "2/1/1"

step "fim"
ok "v1.0-RC3: a história da autorização é verificável do início ao desfecho"
echo "   evidências em $OUT"

#!/usr/bin/env bash
# Aletheia v1.0-RC1 — demonstração ponta a ponta, num comando.
#
#   scripts/rc-smoke.sh
#
# Constrói o compilador e o serviço local, e mostra, com processos e disco de verdade:
#   1. compilação e verificação do programa examples/rc1/treasury.ath
#   2. uma execução legítima (intenção confirmada → efeitos no serviço)
#   3. uma tentativa adversarial recusada (o estado do serviço não muda)
#   4. interrupção real (SIGKILL) e recuperação por outro processo, nas três classes de provedor
#   5. o estado persistente antes e depois de cada passo
#   6. as evidências: WAL, journal, concessões consumidas
#   7. um WAL ilegível ou corrompido interrompe a recuperação (não vira "nada a fazer")
#
# Requisitos: Rust (cargo) e um compilador C (o SQLite é compilado junto). Nada de rede além do
# crates.io na primeira compilação; nenhum modelo pago: o modelo é um roteiro local.
# Cada verificação que falha encerra o script com código 1.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OUT="${RC_SMOKE_DIR:-$ROOT/target/rc-smoke}"
PROG=examples/rc1/treasury.ath
FX=examples/rc1/fixtures
REQUEST='"Pague 120.00 a ACME (NF 4471) e 75.50 a BETA; agende 250.00 para BETA em 2026-12-01."'

step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
ok()   { printf '   \033[32m✓\033[0m %s\n' "$*"; }
die()  { printf '   \033[31m✗ %s\033[0m\n' "$*"; exit 1; }
# check "descrição" valor-obtido valor-esperado
check() { if [ "$2" = "$3" ]; then ok "$1: $2"; else die "$1: obtido \`$2\`, esperado \`$3\`"; fi; }

step "0. construção"
cargo build --release --quiet -p athc -p ath-world
ATHC="$ROOT/target/release/athc"
WORLD="$ROOT/target/release/ath-world"
ok "athc e ath-world construídos"

rm -rf "$OUT"; mkdir -p "$OUT"
DB="$OUT/bank.sqlite"
get() { "$WORLD" state --db "$DB" --get "$1"; }
counts() { get counts; }

"$WORLD" init --db "$DB" --seed "$FX/seed.json"
"$WORLD" serve --db "$DB" --port-file "$OUT/addr" > "$OUT/world.out" 2>&1 &
WORLD_PID=$!
trap 'kill "$WORLD_PID" 2>/dev/null || true' EXIT
for _ in $(seq 1 100); do [ -s "$OUT/addr" ] && break; sleep 0.05; done
[ -s "$OUT/addr" ] || die "o serviço não subiu"
ADDR="$(cat "$OUT/addr")"
ok "serviço local em $ADDR · banco $DB (processo $WORLD_PID)"

# run <execução> <roteiro do modelo> [opções extras do athc run]
run() {
  local id="$1" models="$2"; shift 2
  "$ATHC" run "$PROG" --provider scripted --models "$FX/$models" \
    --host "tcp://$ADDR" --trusted "$FX/trusted.json" --timeout 2 \
    --run-id "$id" --grants "$OUT/$id.grants.json" --wal "$OUT/$id.wal" \
    --journal "$OUT/$id.journal.jsonl" --report "$OUT/$id.report.json" \
    --arg "$REQUEST" "$@"
}
grant() { "$ATHC" grant --run "$1" --origin "confirmation:rc-smoke" --out "$OUT/$1.grants.json" --from "$FX/$2"; }
recover() { local id="$1"; shift; "$ATHC" recover "$PROG" --wal "$OUT/$id.wal" --host "tcp://$ADDR" --timeout 2 --report "$OUT/$id.recover.json" "$@"; }

step "1. compilação e verificação"
"$ATHC" check "$PROG"
"$ATHC" obligations "$PROG"
[ "$("$ATHC" obligations "$PROG" | grep -c 'descarregada')" = 3 ] || die "os três contratos deveriam estar descarregados"
ok "os contratos de pay, schedule e wire são conferidos no refine, antes de qualquer efeito"

step "2. estado inicial do serviço (lido do SQLite por outro processo)"
get accounts
check "saldo da tesouraria" "$(get accounts.TREASURY)" "10000.00"

step "3. execução legítima: a intenção confirmada vira concessões, e as concessões viram efeitos"
grant legit intent.json
run legit models-legit.json
check "saldos" "$(get accounts)" '{"ACME":"120.00","BETA":"75.50","MALLORY":"0.00","TREASURY":"9804.50"}'
check "lançamentos" "$(counts)" '{"pays_posted":2,"pays_refunded":0,"receipts":1,"scheduled":1,"wires":0}'
check "concessões consumidas (no disco)" "$(wc -l < "$OUT/legit.grants.json.spent" | tr -d ' ')" "3"
"$ATHC" verify-journal "$OUT/legit.journal.jsonl"
grep -c '"kind":"authorized"' "$OUT/legit.journal.jsonl" | { read -r n; check "efeitos com a concessão registrada no journal" "$n" "3"; }
recover legit --inspect
AFTER_LEGIT="$(counts)"

step "4. tentativa adversarial: instrução injetada no extrato + recombinação de valores e operações"
grant attack intent.json
run attack models-attack.json
check "o estado do serviço não mudou" "$(counts)" "$AFTER_LEGIT"
check "nada para MALLORY" "$(get accounts.MALLORY)" "0.00"
check "nenhuma intenção saiu do processo" "$(grep -c '"kind":"intent"' "$OUT/attack.wal" || true)" "0"
[ ! -e "$OUT/attack.grants.json.spent" ] || die "uma concessão foi consumida sem efeito"
ok "nenhuma concessão consumida"

step "5a. SIGKILL depois do envio de um pagamento (provedor idempotent, dentro da saga)"
grant kill-pay intent.json
set +e; run kill-pay models-legit.json --kill-at sent:1 > "$OUT/kill-pay.out" 2>&1; code=$?; set -e
check "o processo morreu por SIGKILL (128+9)" "$code" "137"
check "o pagamento chegou ao serviço antes da morte" "$(get counts.pays_posted)" "3"
recover kill-pay --inspect && die "deveria haver pendência" || check "inspeção: há obrigação em aberto (saída 3)" "$?" "3"
recover kill-pay
check "reenvio com a mesma chave: um lançamento só para kill-pay:k1" \
  "$("$WORLD" state --db "$DB" | grep -c '"key": "kill-pay:k1"')" "1"
check "a saga interrompida foi estornada" "$(get counts.pays_refunded)" "1"
check "saldos de volta" "$(get accounts.TREASURY)" "9804.50"
recover kill-pay
ok "recuperar de novo não faz nada"

step "5b. SIGKILL depois do envio do agendamento (provedor queryable: consulta, nunca reenvio)"
grant kill-sched intent.json
set +e; run kill-sched models-legit.json --kill-at sent:3 > "$OUT/kill-sched.out" 2>&1; code=$?; set -e
check "o processo morreu por SIGKILL" "$code" "137"
recover kill-sched
check "um agendamento a mais, não dois" "$(get counts.scheduled)" "2"
check "os pagamentos do lote ficaram (a saga tinha terminado)" "$(get counts.pays_posted)" "4"

step "5c. SIGKILL externo enquanto o serviço segura a resposta (kill -9 vindo de fora)"
"$WORLD" fault --db "$DB" --tool receipt --kind hang --ms 3000
grant kill-ext intent.json
run kill-ext models-legit.json > "$OUT/kill-ext.out" 2>&1 &
PID=$!
for _ in $(seq 1 200); do [ "$(get counts.receipts)" = 2 ] && break; sleep 0.05; done
check "o comprovante está no banco e o processo ainda espera a resposta" "$(get counts.receipts)" "2"
# `run … &` roda a função num subshell. Conforme a versão do bash, $! é o próprio athc (o
# subshell faz exec) ou é o subshell com o athc como filho. O sinal tem de chegar ao athc
# (v1.0-RC2: no segundo caso ele ia só para o subshell e o athc seguia vivo durante a
# recuperação; a trava do WAL, W0006, foi quem mostrou).
TARGET="$(pgrep -P "$PID" -x athc || true)"
kill -9 "${TARGET:-$PID}"; set +e; wait "$PID" 2>/dev/null; code=$?; set -e
check "o processo morreu por SIGKILL" "$code" "137"
recover kill-ext
check "um comprovante a mais, não dois" "$(get counts.receipts)" "2"
"$WORLD" log --db "$DB" | grep 'receipt kill-ext:k4'

step "5d. SIGKILL depois do envio de uma remessa (provedor opaque): escalada, não adivinhada"
grant kill-wire intent-wire.json
set +e; run kill-wire models-wire.json --kill-at sent:2 > "$OUT/kill-wire.out" 2>&1; code=$?; set -e
check "o processo morreu por SIGKILL" "$code" "137"
check "a remessa aconteceu no serviço" "$(get counts.wires)" "1"
set +e; recover kill-wire; code=$?; set -e
check "a recuperação sai com 3: exige intervenção" "$code" "3"
check "a remessa não foi reenviada" "$(get counts.wires)" "1"
grep -q '"kind":"escalate"' "$OUT/kill-wire.wal" || die "a obrigação opaca deveria estar escalada no WAL"
ok "o WAL registra a escalada; nenhum desfecho foi inventado"

step "6. um WAL que não pode ser lido interrompe a recuperação"
grant kill-wal intent.json
set +e; run kill-wal models-legit.json --kill-at sent:3 > "$OUT/kill-wal.out" 2>&1; set -e
SCHED="$(get counts.scheduled)"
cp "$OUT/kill-wal.wal" "$OUT/kill-wal.wal.good"
CALLS="$("$WORLD" log --db "$DB" | wc -l | tr -d ' ')"
# corrupção no meio: uma linha inteira adulterada
sed '2s/"saga":1/"saga":7/' "$OUT/kill-wal.wal.good" > "$OUT/kill-wal.wal"
set +e; recover kill-wal 2> "$OUT/kill-wal.err"; code=$?; set -e
cat "$OUT/kill-wal.err"
check "WAL corrompido: saída 2" "$code" "2"
grep -q W0002 "$OUT/kill-wal.err" || die "faltou o diagnóstico W0002"
# ilegível: um diretório no lugar do arquivo
rm "$OUT/kill-wal.wal"; mkdir "$OUT/kill-wal.wal"
set +e; recover kill-wal 2> "$OUT/kill-wal.err"; code=$?; set -e
cat "$OUT/kill-wal.err"
check "WAL ilegível: saída 2" "$code" "2"
grep -q W0001 "$OUT/kill-wal.err" || die "faltou o diagnóstico W0001"
rmdir "$OUT/kill-wal.wal"
set +e; recover kill-wal 2> "$OUT/kill-wal.err"; code=$?; set -e
cat "$OUT/kill-wal.err"
check "WAL ausente: saída 2" "$code" "2"
check "nenhuma dessas tentativas enviou algo ao serviço" "$("$WORLD" log --db "$DB" | wc -l | tr -d ' ')" "$CALLS"
# o WAL íntegro com uma última linha interrompida: tolerado e reportado
{ cat "$OUT/kill-wal.wal.good"; printf '{"detail":{"key":"k9","tool":"pa'; } > "$OUT/kill-wal.wal"
recover kill-wal
grep -q 'recovery:status' "$OUT/kill-wal.wal" || die "a obrigação em aberto deveria ter sido resolvida por consulta"
check "a obrigação que o WAL guardava foi resolvida sem reenviar o agendamento" "$(get counts.scheduled)" "$SCHED"

step "7. estado final e evidências"
get accounts
counts
check "a partida dobrada fecha (soma dos saldos)" \
  "$("$WORLD" state --db "$DB" | awk -F'"' '/"(ACME|BETA|MALLORY|TREASURY)":/ {s += $4} END {printf "%.2f", s}')" "10000.00"
ls "$OUT" | sed 's/^/   /'
printf '\n\033[1;32mv1.0-RC1: demonstração concluída — todas as verificações passaram\033[0m\n'
printf 'artefatos em %s\n' "$OUT"

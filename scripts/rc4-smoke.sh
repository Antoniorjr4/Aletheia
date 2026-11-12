#!/usr/bin/env bash
# Aletheia v1.0-RC4 — autoridade temporal e ciclo de vida verificado do efeito, num comando.
#
#   scripts/rc4-smoke.sh
#
# Constrói o compilador e o serviço local e mostra, com processos, disco e sinais de verdade,
# sobre examples/rc4/treasury.ath (autoridade tipada e temporal):
#   1. a linguagem: o nível de garantia de cada autoridade; E0427, E0428, E0429, E0430
#   2. uma execução e o ciclo de vida ancorado de cada efeito (reserve → send → outcome)
#   3. o relógio local atrasado duas horas não revalida concessões expiradas
#   4. a testemunha de tempo verificada: repetição, retrocesso, atraso, congelamento
#   5. o prazo que passa entre a reserva e o envio: a reserva é liberada, nada sai
#   6. o WAL que perde os desfechos: restaurados da âncora, sem consultar o provedor;
#      o WAL com um desfecho alterado: recusado (W0008)
#   7. SIGKILL entre a reserva e a autorização de envio: liberada; a concessão volta a valer
#   8. o provedor opaco depois do envio possível: o estado desconhecido é preservado
#
# Três papéis, dois processos: o provedor (o banco, que assina os recibos) e a âncora (que é
# também a testemunha de tempo), em bancos SQLite separados.
# Requisitos: Rust (cargo) e um compilador C. Nenhum modelo pago: o modelo é um roteiro local.
# As chaves usadas são os vetores de teste 1, 2 e 3 da RFC 8032 (públicas, de demonstração).
# Cada verificação que falha encerra o script com código 1.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
OUT="${RC4_SMOKE_DIR:-$ROOT/target/rc4-smoke}"
PROG=examples/rc4/treasury.ath
FX=examples/rc1/fixtures
KEY=examples/rc2/fixtures/DEMO-rfc8032.key
WK=examples/rc4/fixtures
REQUEST='"Pague 120.00 a ACME (NF 4471) e 75.50 a BETA; agende 250.00 para BETA em 2026-12-01."'

step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
ok()   { printf '   \033[32m✓\033[0m %s\n' "$*"; }
die()  { printf '   \033[31m✗ %s\033[0m\n' "$*"; exit 1; }
check() { if [ "$2" = "$3" ]; then ok "$1: $2"; else die "$1: obtido \`$2\`, esperado \`$3\`"; fi; }
# blocked "descrição" código comando…: a saída cita o código e nenhum efeito sai
blocked() {
  local what="$1" code="$2"; shift 2
  local before; before="$(effects)"
  "$@" > "$OUT/last.out" 2>&1 || true
  grep -q "$code" "$OUT/last.out" || { cat "$OUT/last.out"; die "$what: esperado $code"; }
  [ "$(effects)" = "$before" ] || die "$what: um efeito saiu"
  ok "$what: $(grep -o "$code[^\"]*" "$OUT/last.out" | head -1 | cut -c1-120)"
}

step "0. construção"
cargo build --release --quiet -p athc -p ath-world
ATHC="$ROOT/target/release/athc"
WORLD="$ROOT/target/release/ath-world"
rm -rf "${OUT:?}"; mkdir -p "$OUT"
cc -shared -fPIC -o "$OUT/clockshim.so" crates/athc/tests/clockshim.c -ldl
ok "athc, ath-world e o deslocador de relógio construídos"

PIDS=()
trap 'for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null || true; done' EXIT
# world <nome>: um provedor e uma âncora novos
world() {
  for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null || true; wait "$p" 2>/dev/null || true; done
  PIDS=()
  DB="$OUT/$1.bank.sqlite"; ADB="$OUT/$1.anchor.sqlite"
  : > "$OUT/addr"; : > "$OUT/aaddr"
  "$WORLD" init --db "$DB" --seed "$FX/seed.json" > /dev/null
  "$WORLD" serve --db "$DB" --port-file "$OUT/addr" --sign-key "$WK/DEMO-bank.key" > "$OUT/$1.bank.out" 2>&1 & PIDS+=($!)
  "$WORLD" serve --db "$ADB" --port-file "$OUT/aaddr" --sign-key "$WK/DEMO-notary.key" > "$OUT/$1.anchor.out" 2>&1 & PIDS+=($!)
  for _ in $(seq 1 100); do [ -s "$OUT/addr" ] && [ -s "$OUT/aaddr" ] && break; sleep 0.05; done
  [ -s "$OUT/addr" ] && [ -s "$OUT/aaddr" ] || die "os serviços não subiram"
  ADDR="$(cat "$OUT/addr")"; ANCHOR="tcp://$(cat "$OUT/aaddr")"
}
get() { "$WORLD" state --db "$DB" --get "$1"; }
lc() { "$WORLD" state --db "$ADB" --get "lifecycle.$1" | sed 's/null/0/'; }
effects() { echo "$(get counts.pays_posted)/$(get counts.scheduled)/$(get counts.receipts)"; }
NOW="$(date +%s)"

# authorize <execução> [emissão] [validade] [pedido]: concessões, fotografia e pedido, assinados
# PARA este programa, PARA a execução, com prazo
authorize() {
  local id="$1" issued="${2:-$NOW}" ttl="${3:-600}" intent="${4:-intent.json}"
  echo '{"grants": []}' > "$OUT/$id.grants.json"
  "$ATHC" grant --run "$id" --origin "confirmation:rc4-smoke" --out "$OUT/$id.grants.json" --from "$FX/$intent" \
    --sign "$KEY" --program "$PROG" --issued "$issued" --ttl "$ttl" > /dev/null
  cp "$FX/trusted.json" "$OUT/$id.trusted.json"
  "$ATHC" sign --key "$KEY" --trusted "$OUT/$id.trusted.json" --run "$id" --program "$PROG" --ttl 600 > /dev/null
  "$ATHC" sign --key "$KEY" --run "$id" --arg "$REQUEST" --program "$PROG" --ttl 600 --out "$OUT/$id.args.sig"
}
# run <execução> <wal> [opções]
run() {
  local id="$1" wal="$2"; shift 2
  ${PRE:-} "$ATHC" run "$PROG" --provider scripted --models "$FX/${MODELS:-models-legit.json}" \
    --host "tcp://$ADDR" --trusted "$OUT/$id.trusted.json" --timeout 5 --anchor "$ANCHOR" --clock "${CLOCK:-$ANCHOR}" \
    --run-id "$id" --grants "$OUT/$id.grants.json" --wal "$OUT/$wal.wal" \
    --journal "$OUT/$wal.journal.jsonl" --args-sig "$OUT/$id.args.sig" --arg "$REQUEST" "$@"
}
recover() { local wal="$1"; shift; "$ATHC" recover "$PROG" --wal "$OUT/$wal.wal" --host "tcp://$ADDR" --timeout 5 "$@"; }
closes() { python3 -c "
import json,sys
for l in open(sys.argv[1]):
    e=json.loads(l)
    if e['kind'] in ('close','escalate'):
        d=e['detail']; print(d['key'], e['kind'], d.get('status',''), d.get('evidence', d.get('reason','')), (d.get('outcome') or '')[:12])
" "$1"; }

step "1. a linguagem: identidade, contexto, validade, história, tempo e evidência"
sed -n '/^witness/,/^}/p;/^authority/,/^}/p' "$PROG" | sed 's/^/   | /'
"$ATHC" check "$PROG" | tail -1
for p in rc2 rc3; do "$ATHC" check "examples/$p/treasury.ath" | tail -1 | cut -c1-110; done
blocked "--strict-authority na tesouraria da RC3" E0430 "$ATHC" check --strict-authority examples/rc3/treasury.ath
sed 's/ops = \[pay, schedule, wire\]/ops = [pay, schedule, wire, pya]/' "$PROG" > "$OUT/m1.ath"
blocked "operação inexistente" E0427 "$ATHC" check "$OUT/m1.ath"
sed 's/ops = \[pay, schedule, wire\]/ops = [pay, pay, schedule, wire]/' "$PROG" > "$OUT/m2.ath"
blocked "operação repetida" E0428 "$ATHC" check "$OUT/m2.ath"
sed 's/clock = notary/clock = notario/' "$PROG" > "$OUT/m3.ath"
blocked "testemunha não declarada" E0429 "$ATHC" check "$OUT/m3.ath"

step "2. execução, e o ciclo de vida ancorado de cada efeito"
world legit; authorize ok
run ok ok | tail -3 | sed 's/^/   | /'
check "pagamentos / agendamentos / comprovantes" "$(effects)" "2/1/1"
check "registro unificado: reserve / send / outcome" "$(lc reserve)/$(lc send)/$(lc outcome)" "3/3/3"
check "desfechos do WAL que citam o desfecho ancorado" "$(grep '"kind":"close"' "$OUT/ok.wal" | grep -c '"outcome":"')" "3"

step "3. o relógio local atrasado duas horas"
world clock; authorize old "$((NOW - 7200))" 600
PRE="env LD_PRELOAD=$OUT/clockshim.so ATH_FAKE_CLOCK_OFFSET=-7200" \
  blocked "concessões expiradas há 1 h 50 min, relógio do processo atrasado 2 h" "expirou" run old old
check "reservas no registro unificado" "$(lc reserve)" "0"

step "4. a testemunha de tempo é verificada"
for spec in "replay 2 0 T0001 repetição" "skew 3 -100 T0002 retrocesso" "hang 3 3000 T0003 atraso"; do
  set -- $spec
  world "time-$1"; authorize t
  "$WORLD" fault --db "$ADB" --tool time --kind "$1" --nth "$2" --ms "$3" > /dev/null
  blocked "$5" "$4" run t "t-$1"
done
world time-freeze; authorize t
"$WORLD" fault --db "$ADB" --tool time --kind freeze --nth 3 > /dev/null
"$WORLD" fault --db "$DB" --tool statement --kind hang --ms 4000 > /dev/null
blocked "congelamento (o relógio monotônico andou 4 s, a testemunha não)" T0004 run t t-freeze
check "autorizações de envio" "$(lc send)" "0"

step "5. o prazo passa entre a reserva e a autorização de envio"
world deadline; authorize d
"$WORLD" fault --db "$ADB" --tool time --kind skew --nth 4 --ms 1000 > /dev/null
blocked "revalidação imediatamente antes do efeito" R0638 run d d
check "registro unificado: reserve / release / send" "$(lc reserve)/$(lc release)/$(lc send)" "1/1/0"

step "6. o WAL perde os desfechos; o WAL com um desfecho alterado"
world wal; authorize w
run w w > /dev/null
cp "$OUT/w.wal" "$OUT/w.wal.real"
head -n 1 "$OUT/w.wal.real" > "$OUT/w.wal"
before="$("$WORLD" log --db "$DB" | wc -l)"
recover w | grep -E '●|nada pendente' | sed 's/^/   | /'
check "requisições ao provedor durante a recuperação" "$(( $("$WORLD" log --db "$DB" | wc -l) - before ))" "0"
closes "$OUT/w.wal" | sed 's/^/   | /'
# o close do agendamento trocado para "falhou", com a cadeia do WAL refeita (o WAL inteiro é
# do adversário): a cadeia confere; o desfecho ancorado, não
PYTHONPATH=runtimes/python python3 - "$OUT/w.wal.real" "$OUT/w.wal" <<'PY'
import json, sys
from aletheia_runtime.journal import Journal
es = [json.loads(l) for l in open(sys.argv[1])]
i = next(n for n, e in enumerate(es) if e["kind"] == "close" and e["detail"]["key"] == "k3")
j = Journal()
for e in es[:i]:
    j.push(e["t"], e["kind"], e["detail"])
j.push(es[i]["t"], "close", dict(es[i]["detail"], status="failed"))
open(sys.argv[2], "w").write(j.to_jsonl())
PY
blocked "o desfecho do agendamento trocado para \"falhou\" no WAL" W0008 recover w

step "7. SIGKILL entre a reserva e a autorização de envio"
world unsent; authorize u
set +e; run u first --kill-at reserved:1 > "$OUT/u.out" 2>&1; rc=$?; set -e
check "o processo morreu por SIGKILL (137)" "$rc" "137"
check "requisições do pagamento que chegaram ao provedor" "$("$WORLD" log --db "$DB" | grep -c 'u:k1' || true)" "0"
recover first | grep -E 'k1|nada pendente' | sed 's/^/   | /'
check "liberações no registro unificado" "$(lc release)" "1"
run u second > /dev/null
check "nova execução com as mesmas concessões: pagamentos / agendamentos / comprovantes" "$(effects)" "2/1/1"

step "8. o provedor opaco depois do envio possível"
world opaque; authorize o "$NOW" 600 intent-wire.json
set +e; MODELS=models-wire.json run o o --kill-at sent:2 > "$OUT/o.out" 2>&1; set -e
wires="$(get counts.wires)"
set +e; recover o > "$OUT/o.recover" 2>&1; rc=$?; set -e
grep -E 'k2' "$OUT/o.recover" | head -2 | sed 's/^/   | /'
check "athc recover: exige intervenção (3)" "$rc" "3"
check "remessas reenviadas pela recuperação" "$(( $(get counts.wires) - wires ))" "0"

step "fim"
ok "v1.0-RC4: autoridade temporal e ciclo de vida verificado, demonstrados"
echo "   evidências em $OUT"

#!/usr/bin/env bash
# Aletheia v1.0-RC9 — a triagem pelo servidor MCP OFICIAL do GitHub, contra o sandbox.
#
#   GH_TOKEN=… SANDBOX_REPO=dono/repositório scripts/rc9-github-mcp.sh
#
# O servidor é o binário publicado de github/github-mcp-server (versão fixada em MCP_VERSION,
# soma SHA-256 conferida contra o checksums.txt do release), iniciado pelo executor por stdio,
# com `--toolsets=issues --features=ifc_labels`. Nada do servidor é código da Aletheia.
#   1. três issues de teste: A e B (no cadastro) e uma canário C (fora dele)
#   2. a conferência na ligação contra o servidor real: um programa que declara `Int` para o
#      `id` do comentário (o servidor o anuncia como texto) é recusado antes de qualquer efeito
#   3. o rótulo do servidor só rebaixa: um programa que declara confiável o título de uma issue
#      é recusado na leitura (R0649) num repositório público (o do próprio projeto, só lido), e
#      no sandbox, se ele é privado, nada muda
#   4. a triagem SOB SUPERVISÃO, morta (SIGKILL) depois de enviar o primeiro comentário: o
#      supervisor a retoma, o servidor é reiniciado e reconferido, a consulta acha o comentário
#      pelo marcador, e no GitHub fica um comentário em A, um em B, nenhum em C, e uma issue de
#      resumo que cita exatamente #A e #B
#   5. o token não está em arquivo algum; as issues de teste são fechadas
#
# Requisitos: Rust (cargo), `gh`, curl e python3. O token só precisa de issues do sandbox; o
# download do servidor usa GITHUB_TOKEN_DOWNLOAD (ou GH_TOKEN). Modelo roteirizado; nada pago.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
: "${GH_TOKEN:?defina GH_TOKEN (token do repositório de sandbox)}"
: "${SANDBOX_REPO:?defina SANDBOX_REPO=dono/repositório}"
export GH_TOKEN
MCP_VERSION="${MCP_VERSION:-v2.1.0}"
OUT="${RC9_GITHUB_DIR:-$ROOT/target/rc9-github-mcp}"
PROG=examples/rc9/triage.ath
STAMP="$(date +%s)-$$"

step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
ok()   { printf '   \033[32m✓\033[0m %s\n' "$*"; }
die()  { printf '   \033[31m✗ %s\033[0m\n' "$*"; exit 1; }
check() { if [ "$2" = "$3" ]; then ok "$1: $2"; else die "$1: obtido \`$2\`, esperado \`$3\`"; fi; }
api() { gh api "$@"; }

step "0. construção e o servidor MCP oficial do GitHub ($MCP_VERSION)"
cargo build --release --quiet -p athc
ATHC="$ROOT/target/release/athc"
rm -rf "${OUT:?}"; mkdir -p "$OUT/bin"
GH_TOKEN="${GITHUB_TOKEN_DOWNLOAD:-$GH_TOKEN}" gh release download "$MCP_VERSION" -R github/github-mcp-server \
  -p 'github-mcp-server_Linux_x86_64.tar.gz' -p '*checksums.txt' -D "$OUT/bin"
(cd "$OUT/bin" && sha256sum -c --ignore-missing ./*checksums.txt) | sed 's/^/   /'
tar -xzf "$OUT/bin/github-mcp-server_Linux_x86_64.tar.gz" -C "$OUT/bin" github-mcp-server
BIN="$OUT/bin/github-mcp-server"
ok "$("$BIN" --version 2>&1 | head -1)"
export GITHUB_MCP_SERVER="$BIN stdio --toolsets=issues --features=ifc_labels"
export GITHUB_OWNER="${SANDBOX_REPO%%/*}" GITHUB_REPO="${SANDBOX_REPO#*/}"
"$ATHC" check "$PROG" | grep "ligação" | sed 's/^/   /'

CREATED=()
cleanup() {
  for n in "${CREATED[@]}"; do api -X PATCH "repos/$SANDBOX_REPO/issues/$n" -f state=closed --silent 2>/dev/null || true; done
}
trap cleanup EXIT

step "1. as issues de teste no sandbox"
new_issue() { api "repos/$SANDBOX_REPO/issues" -f title="$1" -f body="Issue de teste criada por scripts/rc9-github-mcp.sh." --jq .number; }
A="$(new_issue "RC9 $STAMP: bomba P-101 com vibração alta")"
B="$(new_issue "RC9 $STAMP: vazamento no selo da P-205")"
C="$(new_issue "RC9 $STAMP: canário — nenhuma nota deve chegar aqui")"
CREATED+=("$A" "$B" "$C")
ok "issues #$A e #$B (cadastro), canário #$C"
echo "{ \"inventories\": { \"issues\": [\"$A\", \"$B\"] } }" > "$OUT/trusted.json"
note() { echo "{ \"kind\": \"Note\", \"issue\": \"$1\", \"text\": \"$2\" }"; }
echo "{ \"planner\": [ { \"kind\": \"Plan\", \"notes\": [ $(note "$A" 'Próximo passo: medir a vibração e comparar com a linha de base.'), $(note "$C" CANARIO), $(note "$B" 'Próximo passo: trocar o selo mecânico.') ] } ] }" > "$OUT/models.json"

go() {
  local sub="$1" prog="$2" id="$3"; shift 3
  local idflag=(); [ "$sub" != resume ] && idflag=(--run-id "$id")
  "$ATHC" "$sub" "$prog" --provider scripted --models "$OUT/models.json" --trusted "$OUT/trusted.json" \
    --timeout 30 "${idflag[@]}" --wal "$OUT/$id.wal" --journal "$OUT/$id.$sub.jsonl" \
    --report "$OUT/$id.$sub.report.json" --arg "\"$A\"" --arg "\"$B\"" "$@"
}
comments() { api "repos/$SANDBOX_REPO/issues/$2/comments?per_page=100" --jq "[.[] | select(.body | contains(\"aletheia:$1:\"))] | length"; }
summaries() { api "repos/$SANDBOX_REPO/issues?state=open&per_page=100" --jq "[.[] | select((.body // \"\") | contains(\"aletheia:$1:\"))] | length"; }
state() { echo "$(comments "$1" "$A")/$(comments "$1" "$B")/$(comments "$1" "$C")/$(summaries "$1")"; }
settled() { local s; for _ in $(seq 1 20); do s="$(state "$1")"; [ "$s" = "$2" ] && break; sleep 3; done; echo "$s"; }

step "2. a conferência na ligação, contra o servidor real"
R0="rc9-$STAMP-int"
sed -e 's/k: own IdemKey) -> Text\[..200\] ^ Untrusted/k: own IdemKey) -> Int[1..9000000000000]/' \
    -e 's/body: body) -> html_url"/body: body) -> id"/' "$PROG" > "$OUT/id-int.ath"
set +e; go run "$OUT/id-int.ath" "$R0" > "$OUT/id-int.out" 2>&1; code=$?; set -e
grep -E "R0646" "$OUT/id-int.out" | sed 's/^/   /' | cut -c1-220
[ "$code" != 0 ] && grep -q "R0646" "$OUT/id-int.out" || { cat "$OUT/id-int.out"; die "esperada a recusa R0646"; }
grep -q "string" "$OUT/id-int.out" || die "a recusa não diz o tipo que o servidor anuncia"
sleep 3
check "nada saiu (comentários em #$A / #$B / canário #$C / resumos)" "$(state "$R0")" "0/0/0/0"

step "3. o rótulo do servidor só rebaixa"
# probe <dono/repo> <issue> <rótulo>: só lê o título, declarado confiável
probe() {
  local repo="$1" n="$2" tag="$3" private code
  private="$(api "repos/$repo" --jq .private)"
  set +e
  GITHUB_OWNER="${repo%%/*}" GITHUB_REPO="${repo#*/}" "$ATHC" run examples/rc9/label-probe.ath --timeout 30 \
    --run-id "rc9-$STAMP-probe-$tag" --wal "$OUT/probe-$tag.wal" --journal "$OUT/probe-$tag.jsonl" \
    --arg "\"$n\"" > "$OUT/probe-$tag.out" 2>&1; code=$?
  set -e
  grep -oE "R0649[^;]*|resultado: .*" "$OUT/probe-$tag.out" | head -1 | sed 's/^/   /' | cut -c1-200
  if [ "$private" = false ]; then
    [ "$code" != 0 ] && grep -q "R0649" "$OUT/probe-$tag.out" || { cat "$OUT/probe-$tag.out"; die "$repo é público: esperado R0649"; }
    ok "$repo (público): o servidor rotula o título como não confiável, e a declaração confiável é recusada"
  else
    [ "$code" = 0 ] || { cat "$OUT/probe-$tag.out"; die "$repo é privado: o rótulo do servidor é confiável e nada devia mudar"; }
    ok "$repo (privado): o servidor rotula o título como confiável, e nada muda"
  fi
}
probe "$SANDBOX_REPO" "$A" sandbox
probe "${PUBLIC_REPO:-${GITHUB_REPOSITORY:-Antoniorjr4/Aletheia}}" "${PUBLIC_ISSUE:-1}" public

step "4. a triagem sob supervisão, morta depois de enviar o primeiro comentário"
R1="rc9-$STAMP-sup"
set +e
go supervise "$PROG" "$R1" --inject-kills sent:1 --backoff 1 --max-resumes 4 > "$OUT/sup.out" 2>&1; code=$?
set -e
grep -E "supervisor:|resultado" "$OUT/sup.out" | sed 's/^/   /' | cut -c1-160
grep -oE "ligada\(s\) por MCP \(stdio\) a [^;]*|conferidas contra o servidor [^;]*|[0-9]+ pedido\(s\) do servidor ao cliente recusado\(s\)[^;]*" "$OUT/sup.out" | sort -u | sed 's/^/   /' 
check "o supervisor terminou (0 = concluído)" "$code" "0"
check "no GitHub (comentários em #$A / #$B / canário #$C / resumos)" "$(settled "$R1" 1/1/0/1)" "1/1/0/1"
SUMMARY="$(api "repos/$SANDBOX_REPO/issues?state=open&per_page=100" --jq ".[] | select((.body // \"\") | contains(\"aletheia:$R1:\")) | {number, title, body}")"
for n in $(echo "$SUMMARY" | python3 -c "import json,sys; [print(json.loads(l)['number']) for l in sys.stdin if l.strip()]"); do CREATED+=("$n"); done
check "a issue de resumo cita" "$(echo "$SUMMARY" | python3 -c "
import json, re, sys
t = ' '.join(re.sub(r'<!--.*?-->', ' ', (d.get('title') or '') + ' ' + (d.get('body') or ''), flags=re.S) for d in map(json.loads, filter(str.strip, sys.stdin)))
print(','.join('#' + n for n in sorted(set(re.findall(r'#([0-9]+)', t)), key=int)))")" "#$A,#$B"

step "5. o token não está em arquivo algum"
set +e; grep -rqa --exclude-dir=bin -- "$GH_TOKEN" "$OUT"; code=$?; set -e
case "$code" in
  1) ok "nenhuma ocorrência do token em $OUT" ;;
  0) die "o token está em disco" ;;
  *) die "a busca pelo token falhou (grep $code)" ;;
esac

printf '\n\033[1;32mv1.0-RC9 (servidor MCP do GitHub): todas as conferências passaram.\033[0m\n'

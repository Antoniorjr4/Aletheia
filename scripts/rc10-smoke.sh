#!/usr/bin/env bash
# Aletheia v1.0-RC10 — confidencialidade: o fluxo tóxico barrado por tipo, sem rede.
#
#   scripts/rc10-smoke.sh
#
#   1. a triagem legítima (examples/rc10/triage.ath) passa: canais públicos recebem dado público
#   2. o fluxo tóxico (examples/rc10/leak.ath) é recusado na verificação (E0440), nada roda
#   3. derivar do segredo não o "lava": concatenar e compor continuam Secret
#   4. em runtime, o rótulo do servidor MCP só eleva: um título que o programa declarou publicável
#      e o servidor marca secreto não entra (R0648)
# Requisitos: Rust (cargo) e python3. Nenhuma rede, nenhum modelo pago.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
OUT="${RC10_SMOKE_DIR:-$ROOT/target/rc10-smoke}"
step(){ printf '\n\033[1m== %s\033[0m\n' "$*"; }
ok(){ printf '   \033[32m✓\033[0m %s\n' "$*"; }
die(){ printf '   \033[31m✗ %s\033[0m\n' "$*"; exit 1; }

step "0. construção"
cargo build --release --quiet -p athc -p ath-world
ATHC="$ROOT/target/release/athc"; WORLD="$ROOT/target/release/ath-world"
rm -rf "${OUT:?}"; mkdir -p "$OUT"

step "1. a triagem legítima passa"
"$ATHC" check examples/rc10/triage.ath | grep -E "E3 fluxo|ligação" | sed 's/^/   /'
"$ATHC" check examples/rc10/triage.ath >/dev/null || die "a triagem legítima foi recusada"
ok "canais públicos (comment, open_issue) recebem só dado público"

step "2. o fluxo tóxico é recusado antes de qualquer efeito"
"$ATHC" check examples/rc10/leak.ath > "$OUT/leak.out" 2>&1 && die "o leak foi aceito"
grep -oE "E0440[^;]*" "$OUT/leak.out" | head -1 | sed 's/^/   /'
grep -q E0440 "$OUT/leak.out" && ok "E0440: dado Secret não chega a um canal Public"

step "3. derivar do segredo não o lava"
for body in 's ++ s' '\"vazou: \" ++ s'; do
  cat > "$OUT/d.ath" <<ATH
module d
tool read_private(c: Cap[Repo.Read], i: Text[..8]) -> Text[..200] ^ Secret ! { Repo.Read } reversible latency 1.0
tool post_public(c: Cap[Repo.Write], body: Text[..600] ^ Public) -> Unit ! { Repo.Write } irrevocable latency 1.0
agent A(r: Cap[Repo.Read], w: Cap[Repo.Write]) budget [tok<=10, usd<=1.0, wall<=10.0] {
  on go(n: Text[..8] ^ Trusted) -> Unit ! { Repo.Read, Repo.Write } { let s = read_private(r, n) commit post_public(w, $body) }
}
ATH
  "$ATHC" check "$OUT/d.ath" >/dev/null 2>&1 && die "aceito: $body"
done
ok "concatenação e composição a partir de Secret continuam Secret"

step "4. em runtime, o rótulo do servidor só eleva (R0648)"
DB="$OUT/w.sqlite"
"$WORLD" init --db "$DB" --seed examples/rc1/fixtures/seed.json >/dev/null
python3 - "$DB" <<'PY'
import sqlite3, sys
c=sqlite3.connect(sys.argv[1]); c.execute("INSERT INTO gh_issues (title, body) VALUES ('bomba','')"); c.commit()
PY
GH_TOKEN=tok GITHUB_OWNER=o GITHUB_REPO=r \
GITHUB_MCP_SERVER="$WORLD mcp --db $DB --mode secret-label" \
  "$ATHC" run examples/rc9/label-probe.ath --timeout 5 --run-id p --wal "$OUT/p.wal" --journal "$OUT/p.jsonl" --arg '"1"' > "$OUT/probe.out" 2>&1 && die "o título secreto entrou"
grep -oE "R0648[^;]*" "$OUT/probe.out" | head -1 | sed 's/^/   /'
grep -q R0648 "$OUT/probe.out" && ok "título declarado publicável, servidor diz secreto → não entra"

printf '\n\033[1;32mv1.0-RC10: todas as conferências passaram.\033[0m\n'

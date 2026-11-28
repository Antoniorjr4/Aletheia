# Revisão adversarial do segundo reparo (F1/F2/F3) da análise de obrigações

Alvo: o diff não commitado de `crates/ath-syntax/src/obligations.rs`, na branch `consolidacao/ciclo-1`
(HEAD `f267d44`).

Tudo foi feito localmente. Não usei modelo pago nem `--real`, não disparei workflow e não fiz commit
nem push. O worktree do usuário ficou como estava. Para compilar o HEAD, criei um worktree temporário
(`/tmp/wt-head`) e já o removi.

Compiladores comparados:

| nome | binário | o que é |
|---|---|---|
| **antes** | `/tmp/athc-antes` | antes de B1/B2 |
| **HEAD** | `/tmp/athc-head` | `f267d44`, com B1/B2 e sem este diff |
| **diff** | `target/release/athc` | HEAD com o diff em revisão |
| **fix** | `/tmp/athc-fix` | diff com a correção candidata do fim deste relatório; usado só para validá-la |

Casos novos em `/tmp/rev2/`:
- gerador `gen.py`, com cabeçalho e cauda fixos, mais `cases1.py` e `cases2.py`;
- casos com dois `refine` em `/tmp/rev2/m/`;
- comparação das obrigações entre compiladores: `/tmp/rev2/cmp.sh`.

Mundo `/tmp/rev2/mundo.json`:
- `pairs = [alice|alice, alice|100, bob|5]`;
- `payees = [alice, bob]`.

Para executar cada caso: `ARGS='["\"x\"","false"]' python3 bench/obrigacoes/revisao/run.py <caso> <mundo> <roteiro> [athc…]`.

---

## Achados

### G1: um parâmetro com fontes em campos diferentes do mesmo elemento sai "descarregada" (ALTA; o defeito já existia, e o diff o expõe num caso novo)

**Causa.** Em `site()`, a primeira fonte de um parâmetro num span define a ligação parâmetro ← campo.
Fontes seguintes do mesmo parâmetro, no mesmo span, com mesmo prefixo, mesma variante e mesmo
último id, são aceitas sem comparar o campo:

```rust
if !b.iter().any(|(n, _)| n == name) { b.push((name.to_string(), f.clone())); }
```

O campo da segunda fonte é descartado em silêncio. A obrigação levantada confere `φ` só com o
primeiro campo. Na execução, o outro campo chega ao efeito sem ter sido conferido.

**Reprodução mínima** (`/tmp/rev2/y2_campo_if_um.ath`, roteiro `rot_campo.json` = `[Pay alice/100]`, `c=false`):

```
tool one(c: Cap[Bank.Pay], to: Text ^ Endorsed) -> Unit ! { Bank.Pay } … requires s.member("payees", to)
…
for o in ops { match o { Pay { to, memo } => one(bank, if c { to } else { memo }) } }
```

- Esperado: `dinâmica`, e R0630 no efeito.
- Obtido: `descarregada` (`refine L15 ⇐ one`, com `to ← to`). O ARX e o Python abortam os dois
  com **R0631** "defeito da análise". Com `c=true`, o resultado é `ok`.

Variantes:

| caso | o que faz | antes | HEAD | diff |
|---|---|---|---|---|
| `y1_campo_if_xx` | `let x = if c {to} else {memo}`; `pay(bank, x, x)`. Liga `memo ← to`; com `c=false` chama `pay("100","100")` | R0631 | R0631 | R0631 |
| `y2_campo_if_um` | o caso acima | R0631 | R0631 | R0631 |
| **`y5_campo_dois_elem`** | `for a in ops { for b in ops { let x = if c { a.to } else { b.memo }; one(bank, x) } }` | — | **dinâmica** (correto) | **descarregada → R0631** |
| `m/m6_campo_misto` | dois `refine`; `x` é `a.to`, `b.to` ou `a.memo` | R0631 | R0631 | R0631 |

Sobre `y5`:
- É o **único falso "descarregada" introduzido por este diff**.
- No HEAD, os ids completos `[A]` e `[B]` diferiam. No diff, os dois recebem `[…, IF]` e o último id
  coincide. A relaxação do "último id" não é o defeito em si; ela abre um caminho novo para o G1.

Sobre `m6`:
- No span de `a`, os campos são `to` e `memo`; o segundo é descartado.
- Com dois spans, a ligação por span é correta (`m5_campo_dois_refines` é sólido). O defeito
  aparece só quando há dois campos **no mesmo span**.

Os dois runtimes concordam em todos os casos: o que falha é a análise.

**Correção.** Se o parâmetro já tem ligação no span com outro campo, devolver `Dynamic`. Está em
`/tmp/rev2/fix_candidata.patch` e foi validada; veja a seção "Correção candidata".

### G2: `refine … else { outra proposta }` deixou de descarregar, com mudança de comportamento (MÉDIA, introduzida pelo diff)

**Causa.** A nova regra para mais de um `refine` exige um último id comum e não `None`. O próprio
`refine … else` junta duas origens: a proposta ou o valor do `else`. Mas ele não é tratado como
ponto de escolha e não marca as origens. O padrão de *fallback* fica então sem id comum.

**Reprodução** (`/tmp/rev2/m/m1_else_fallback_op.ath`, roteiro `rot_fb.json` = `[alice/100 (válida), bob/6 (viola)]`):

```
let a = refine (infer planner -> Op {…}) by PO else { return "none" }
let b = refine (infer planner -> Op {…}) by PO else { a }
match b { Pay { to, memo } => pay(bank, to, memo) }
```

- HEAD: `descarregada`, levantada para L13 e L14. A segunda proposta é recusada no `refine`
  (`obligation_reject`), o `else` devolve `a` e sai `pay(alice,100)`. Resultado `ok`. Isso é sólido,
  porque `b` é um valor só.
- Diff: `dinâmica` ("mais de um `refine` alcança a chamada"). A segunda proposta não recebe a
  obrigação e é aceita. No efeito: **R0630** e o handler aborta. Os dois runtimes concordam.

Também:
- `m/m2_else_fallback_laco`: `for o in ops { let q = refine … else { o }; … }`;
- `m/m7_else_if`: `refine … else { if c { a } else { b } }`.

Nenhum programa do corpus usa esse padrão (o corpus não mudou), mas é um padrão realista: uma nova
proposta e, se ela for recusada, a anterior.

**Correção.** Tratar o `refine` como ponto de escolha, com `join([[Refine(sp)], else], sp)`. É
sólido pelo mesmo argumento do `if`: a expressão produz um valor só.

### G3: um `match` de um braço, ou ramos com as mesmas origens, conta como escolha (BAIXA-MÉDIA; vem do B1 e não foi resolvido pelo diff)

**Causa.** `join` decide se há escolha contando as origens distintas da **união**, não comparando os
ramos. Um valor que já tem duas origens (por exemplo, `o = if c { a } else { b }`) recebe um id novo
em cada `match` de um braço que extrai um campo. Com isso, dois campos do mesmo `o` ficam com
últimos ids diferentes.

**Reprodução** (`/tmp/rev2/o1_match_um_braco_join.ath`):

```
for a in ops { for b in ops { let o = if c { a } else { b }
  let t = match o { Pay { to } => to }
  let m = match o { Pay { memo } => memo }
  pay(bank, t, m) } }
```

| antes | HEAD | diff |
|---|---|---|
| descarregada | dinâmica | dinâmica ("`memo` vem de outro elemento") |

O caso é demonstrável. É a forma `let` do F3, que o diff só resolveu na forma com `match o { Pay { to, memo } => … }`.

O mesmo vale para `if c { to } else { to }` quando `to` já tem duas origens: a deduplicação deixa
duas origens, e o valor é marcado.

**Correção.** Só há escolha se os conjuntos de origens dos ramos **diferem**. Um braço só, ou ramos
iguais, não marcam nada (veja o patch).

### G4: reconstrução por `Ctor` juntada ao original vira dinâmica (BAIXA, introduzida)

Caso `/tmp/rev2/o2_reconstroi_join.ath`:

```
let o2 = if c { o } else { Pay { to: to, memo: memo } }
```

- HEAD: descarregada (sólido).
- Diff: dinâmica.

O `join` marca a origem `Refine` do ramo `o`, mas não as origens dentro do literal. Isso é correto:
marcar dentro de um `Lit` seria inseguro, porque um literal pode misturar elementos. Por isso os
últimos ids divergem. É uma limitação inerente ao desenho do "último id". Não há correção simples
sem uma identidade por valor. Registro para documentar.

### G5: menores, já existentes, sem falha de segurança

- `k6_join_listas_mistas`: `if c { ops } else { row.ops }` dá prefixos diferentes no mesmo span →
  dinâmica. Um span só aceita um caminho.
- `o3_campo_ponto`: `o.to` continua "um valor calculado" (F4 da revisão anterior).
- `z15`: `for x in [v] { for y in [v] … }` dá dinâmica, embora `x` e `y` sejam o mesmo `v`.

---

## Ataques à relaxação do "último id": todos se sustentaram

Todos os casos abaixo dão `dinâmica` no diff. Com plano inválido, nenhum dá R0631, e os dois
runtimes concordam. No **antes**, todos davam "descarregada".

| caso | ataque |
|---|---|
| `z1_k_interno` | `for w in [v, a]`, com `v = if c {a} else {b}`: a escolha interna mistura com `v` |
| `z3_dois_if_mesmo_c` | dois `if` com a mesma condição e escolhas cruzadas (spans diferentes) |
| `z4_if_de_if` | `w = if c { v } else { a }` com campos de `w` e de `v` |
| `z5_lista_de_listas` | `grid: List[List[Op]]`, com `m = if c {l} else {l2}` e dois laços sobre `m` |
| `z6_lista_lit_listas` | `for l in [ops, ops]` com dois laços sobre `l` |
| `z7_pair_join` | `Pair { a, b }` com os campos trocados num `if`; o `Lit` não é marcado |
| `z8_join_unknown_variante` | `if c { a } else { Pay { "evil" } }` → Unknown |
| `z9_sombra_join` e `z16_sombra_var_laco` | sombreamento de `v` dentro do braço |
| `z10_lista_join_iter` | `l = if c {[a,b]} else {[b]}` com dois laços sobre `l` |
| `z11_if_dentro_laco_campos` | um `if` por campo dentro do laço (F2 com um `refine` só) |
| `z12_rows_join` | `rr = if c {r} else {r2}` e `r.ops`: diferença externa escondida |
| `z13_lista_join_mesmo_laco` | `v = if c {x} else {a}` com `x ∈ [a,b]` |
| `z14_grade_cruzada_join` | `m = if c {l} else {l2}` contra `l` |
| `m/m4_v_e_a` e `m/m8_dois_if_iguais` | dois `refine`, misturando `v` com `a`, ou `v` com `w` de outro `if` |

Por que a relaxação é sólida, conferido no código:
- Depois do último ponto de escolha K, os únicos passos que mantêm uma origem `Refine` são `Field`
  (comparados no prefixo) e a passagem por `let`, bloco ou `commit`.
- Todo passo `Elem` empilha um id novo, e todo `join` com mais de uma origem também.
- Não há funções nem mutação, e um `for` devolve `Unknown`. Assim, cada K é avaliado no máximo uma vez
  por instância de escopo, e nenhum valor sai de uma iteração.
- O parser cria os spans de `if`, `match` e da variável do `for` a partir de tokens distintos, num
  arquivo só; não achei colisão de span.
- **Ressalva:** a solidez depende dessas propriedades da linguagem. Se entrarem closures, estado
  mutável ou dobras com acumulador no corpo de um handler, a regra quebra. Vale um comentário e um
  teste que fixem essa premissa.

Controles **descarregados e corretos** nos dois runtimes. Com plano válido, as chamadas saem. Com um
elemento inválido, há `obligation_reject` no `refine` e nenhum efeito.
- `k1_if_mesmo_laco`, `k2_ops_join_vazio` (`if c { ops } else { [] }`), `k3_match_campo_unico`;
- `z2_match_v_duas` (`match v {to}` e `match v {memo}` sobre o mesmo `v` juntado);
- `k4_grade` (`grid[*][*]`), `k5_grade_join`, `k8_lista_lit_join` (`for x in (if c {[a]} else {[b,a]})`);
- `o4_let_campos`, `m/m3_if_dois_refines`.

**F1, `free_vars`.** Os casos `c/f1`–`f6` deram `dinâmica` ou uma ligação completa. A divergência
ARX/Python do `f6_parcial` sumiu: antes era R0100 no ARX contra `None`/R0631 no Python; agora os dois
dão `ok` com `rot_ok` e `obligation_reject` com `rot_evil`. Uma forma desconhecida faz todos os
parâmetros contarem, inclusive `Cap`, que é `Unknown` → `dinâmica`. É conservador. Achei correto que
um quantificador ou braço que liga um nome igual a um parâmetro o conte como lido: isso
sobre-aproxima.

---

## Revisão anterior e corpus

**`bench/obrigacoes/revisao/run.py` em todos os casos `c`, `d` e `e`.** São 116 execuções, cada uma
nos dois runtimes:

| compilador | execuções com R0631 | divergências ARX/Python |
|---|---|---|
| antes | 35 | `f6` (2) |
| HEAD | 17 | `f6` (2) |
| diff | **0** | **0** |
| fix | 0 | 0 |

A saída do fix é linha a linha idêntica à do diff.

**`regressao_corpus.py --antes /tmp/athc-antes --depois target/release/athc --out /tmp/rc2.json`.**
- 252 programas, 56 com sítios.
- Sítios descarregados: 63 → 42. Dinâmicos: 3 → 24.
- Só mudaram casos de falha e de revisão: `bench/obrigacoes/casos/*` e `bench/obrigacoes/revisao/*`.
- O `/tmp/rc2.json` é **idêntico** ao `bench/obrigacoes/regressao_corpus.json` modificado no worktree.

**Comparação HEAD → diff** (`/tmp/rc2_head.json`): mudaram 15 arquivos, todos em `bench/obrigacoes/`.
- `c3_*`, `k_if_mesmo_laco` e `k_lista_dois` passaram a "descarregada" (F3 resolvido).
- `f1_*`, `f2_*` e `a1`/`a2` passaram a dinâmica.
- Em `x_if_campos` e `x_lista_cruzada` mudou só o texto do motivo.

**Programas reais** (`examples/`, `bench/agentdojo`, `bench/m10`, `bench/m11`): são 10 programas com
20 sítios, todos descarregados. **Nada mudou** entre antes, HEAD, diff e fix.

**Python emitido** (`emit --target py` dos 252 programas; 115 são aceitos):
- Dos programas reais, nenhum byte diferiu entre antes, HEAD e diff.
- HEAD → diff: 15 diferenças, todas em `bench/obrigacoes/`. São as esperadas: `lifted=[…]` e
  `discharged=True` aparecem ou somem. Em `f1_requires_parcial`/`f6`, a ligação ganhou `(2, "memo")`.
- fix → diff: **0 diferenças**.

**`cargo test --all`** no worktree: **exit 0**. Foram 322 aprovados, 0 falhas e 1 ignorado, em 43
suítes. `ciclo1_obrigacoes` passou os 7 testes. Com o fix, `ciclo1_obrigacoes` (o arquivo de teste
do worktree) e `ath-syntax` também passam.

---

## Correção candidata (G1, G2, G3), validada sem tocar no worktree do usuário

Está em `/tmp/rev2/fix_candidata.patch` (81 linhas, sobre o diff atual).

1. `site()`: se o parâmetro já tem ligação no span com **outro campo**, devolver `Dynamic` ("vem de
   campos diferentes"). Corrige o G1.
2. `join(branches: Vec<Vec<Src>>, at)`: há escolha só se os conjuntos deduplicados dos ramos que
   produzem valor diferem. Um ramo só, ou ramos iguais, não marcam. `return` (vazio) é ignorado.
   Corrige o G3.
3. `Refine`: `join([[Refine(sp)], else], sp)`. Corrige o G2.

Resultado do fix:
- `y1`, `y2`, `y5` e `m6` passam a `dinâmica`.
- `o1`, `m1`, `m2` e `m7` passam a `descarregada`. Com plano válido saem as chamadas; com proposta
  inválida há `obligation_reject` e o `else` devolve o valor anterior. Nenhum R0631, e os runtimes
  concordam.
- O corpus fica sem mudança (`/tmp/rev2/rc_fix.json`) e o Python emitido é idêntico.
- A revisão anterior dá a mesma saída.

O G4 continua; é uma limitação do desenho. Sugiro acrescentar ao `ciclo1_obrigacoes.rs` testes de
regressão para `y2`, `y5` e `m1`.

## Artefatos

- Casos e fixtures: `/tmp/rev2/*.ath`, `/tmp/rev2/m/*.ath`, `/tmp/rev2/*.json`, `/tmp/rev2/m/*.json`.
- Saídas: `/tmp/rev2/revisao_run.txt`, `/tmp/rev2/revisao_run_fix.txt`, `/tmp/rev2/cargo_test.txt`,
  `/tmp/rc2.json`, `/tmp/rc2_head.json`, `/tmp/rev2/rc_fix.json`.
- Comparação do Python emitido: `/tmp/rev2/emit_cmp.py` (`emit/` e `emit_fix/`).
- Binários: `/tmp/athc-head`, `/tmp/athc-fix`.

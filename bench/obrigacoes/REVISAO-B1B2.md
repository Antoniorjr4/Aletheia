# Revisão adversarial independente do reparo B1/B2 (análise de obrigações)

Branch `consolidacao/ciclo-1`, diff não commitado em `crates/ath-syntax/src/obligations.rs`.
Tudo foi feito localmente, sem modelo real, sem `--real`, sem workflow, sem commit e sem push.
O worktree ficou como estava. Os artefatos desta revisão estão em `/tmp/rev/`: o runner genérico é
`/tmp/rev/run.py` e os casos estão em `/tmp/rev/c`, `/tmp/rev/d` e `/tmp/rev/e`.

Compiladores comparados:
- **depois**: `target/release/athc`, com o reparo;
- **antes**: `/tmp/athc-antes`;
- **congelado**: `/tmp/frozen-target/release/athc`, que não aceita a renomeação em padrão
  `Pay { to: a }`; por isso os casos que dependem dele foram escritos sem renomeação quando possível.

Runner: `python3 /tmp/rev/run.py prog.ath mundo.json roteiro.json [athc…]`. Para cada
compilador, ele mostra `athc obligations`, o ARX (`run --provider scripted`) e o Python emitido
(`emit --target py` + `aletheia_runtime run --provider scripted`). Os argumentos do evento vão
em `ARGS='["\"x\"","true"]'`.

---

## Achados

### F1 — `free_vars` é parcial: `requires` com `if`/`match`/quantificador/bloco descarrega qualquer argumento (ALTA, pré-existente)

`free_vars` (obligations.rs:128) não percorre `If`, `Match`, `Block` nem `Quant`. Um parâmetro que
`φ` só lê dentro de uma dessas formas fica de fora de `params`. Se nenhum fica, `params` é vazio,
`per` é vazio e o sítio sai `Discharged { refines: [] }`, isto é, "obrigação vazia". Isso vale para
**argumentos literais, sem `refine` nenhum**. O checker aceita essas formas em `requires`.

Reprodução mínima (`/tmp/rev/c/f2_lit_if.ath`; também `f4_lit_quant.ath` com `all t in ts => …`,
`f5_lit_match.ath` com `match o { Pay { to } => … }` e `f3_lit_block.ath` com `{ … }`):

```
tool pay(c: Cap[Bank.Pay], to: Text[..16], memo: Text[..16]) -> Unit ! { Bank.Pay } reversible latency 1.0
  requires if true { s.member("payees", to) } else { false }
...
    pay(bank, "mallory", "x")
```

- Esperado: `dinâmica`, e R0630 no efeito.
- Obtido: `L10 pay descarregada levantada para o refine em` (lista vazia). ARX e Python abortam
  com **R0631**, nos três compiladores.

**Variante com divergência entre runtimes** (`/tmp/rev/c/f6_parcial.ath`):

```
requires s.member("payees", to) && (if memo == "evil" { false } else { true })
```

`params = [to]`, porque `memo` está dentro do `if`. A obrigação é levantada com a ligação `to`
apenas. Na conferência levantada no `refine`, `memo` não tem valor:
- ARX: aborta com **R0100** "variável `memo` não vinculada", **mesmo com plano válido**
  (`rot_ok.json`, memo="ok"). Não há efeito.
- Python: `memo=None`. Com o plano válido, o pagamento sai e o resultado é "ok". Com
  memo="evil" (`rot_evil.json`), o resultado é **R0631**.

Pré-existente: antes, depois e congelado dão o mesmo resultado. O corpus não exercita o defeito:
todo `requires` versionado é `&&`/`||` de `s.método(…)` (levantamento em `/tmp/rev`).

Reparo sugerido, sem impacto no corpus:
- tornar `free_vars` total (`If`, `Match` com escrutínio e corpos, `Block` com `let`s e cauda,
  `Quant` com `iter` e corpo, `Await`), ou, de forma conservadora, considerar usados todos os
  parâmetros quando aparecer uma forma não tratada;
- como defesa, nunca descarregar com `params` vazio quando `φ` lê algum parâmetro;
- nos dois runtimes, tratar da mesma forma um parâmetro sem ligação na conferência levantada.

### F2 — Correlação perdida entre `refine` diferentes: `if` por parâmetro cruza duas propostas (MÉDIA-ALTA, pré-existente)

`site()` confere a identidade do elemento **por span de `refine`**: em cada span, todos os
argumentos vêm do mesmo (prefixo, ids, variante). Não confere que, **numa mesma execução**, todos
os argumentos vêm do **mesmo** `refine`. Se cada parâmetro é uma junção (`if`/`match`) de campos de
duas propostas, cada span parece completo e o sítio é descarregado. Na execução, porém, um campo
vem de uma proposta e o outro da outra.

Reprodução mínima, sem laços (`/tmp/rev/c/a2_dois_refines_registro.ath`, mundo
`pairs=["alice|100","bob|5"]`, roteiro `[Pay alice/100, Pay bob/5]`, `c=true`):

```
let a = refine (infer planner -> Op { request = req }) by P else { return "none" }
let b = refine (infer planner -> Op { request = req }) by P else { return "none" }
let x = if c { match a { Pay { to } => to } } else { match b { Pay { to } => to } }
let y = if c { match b { Pay { memo } => memo } } else { match a { Pay { memo } => memo } }
pay(bank, x, y)      // requires s.binds("pairs", to, memo)
```

- Esperado: `dinâmica`. `x` e `y` sempre vêm de propostas diferentes.
- Obtido: `descarregada levantada para o refine em L9, L10`. Os dois `refine` aceitam, porque cada
  proposta, sozinha, satisfaz a obrigação. Depois, `pay("alice","5")` → **R0631** no ARX e no
  Python, nos três compiladores.

A variante com listas (`/tmp/rev/c/a1_dois_refines_if.ath`, dois planos e `for a in o1 { for b in o2 … }`)
dá o mesmo resultado. O reparo de B1 não a pega: os ids são comparados só dentro do mesmo span.

Reparo sugerido:
- quando mais de um span alcança o sítio, exigir que todos os argumentos de todos os spans
  compartilhem o mesmo ponto de escolha. Na prática: os mesmos `ids` não vazios, ou seja, o
  elemento foi escolhido por um único laço sobre uma lista que já era a junção;
- senão, `dinâmica`. Um token de origem por junção (`let`/`if`/`match`/`else`) seria mais preciso.

Nenhum sítio descarregado do corpus é alcançado por mais de um `refine`. Conferi o
`regressao_corpus.json` regerado: nenhum "levantada para o refine em Lx, Ly". A regra conservadora
não muda nada no corpus. O caso legítimo `let p = refine … else { p0 }` + `for o in ops { pay(o.to, o.memo) }`
(`/tmp/rev/d/k_else_outro_refine.ath`) usa os mesmos ids `[o]` nos dois spans e continuaria
descarregado.

### F3 — Excesso de conservadorismo introduzido: escolha entre dois elementos do mesmo laço (BAIXA-MÉDIA, introduzido)

Os ids precisam ser iguais em todas as fontes do span. Por isso, um valor que **é** um elemento ou
outro, com os dois campos lidos do mesmo valor, virou `dinâmica`. A origem comum é demonstrável
nos dois casos:

- `/tmp/rev/d/k_if_mesmo_laco.ath`:
  `for a in ops { for b in ops { let o = if c { a } else { b }; match o { Pay { to, memo } => pay(bank, to, memo) } } }`
- `/tmp/rev/d/k_lista_dois.ath`:
  `for a in ops { for b in ops { for x in [a, b] { match x { Pay { to, memo } => pay(…) } } } }`

Antes: `descarregada`. Depois: `dinâmica` ("`to` vem de outro elemento").

O comportamento muda de forma observável. Com o roteiro `rot_inv.json`, em que um elemento viola,
antes o plano era recusado no `refine`, sem efeito. Agora saem 2 ou 3 pagamentos legítimos e depois
vem R0630. Os dois runtimes concordam.

Não há caso do corpus afetado. A causa é que `for x in [a, b]` e o `if` não criam um ponto de
escolha próprio. Um token de identidade criado no `let`/`for` que liga o valor, propagado pelos
campos, resolveria F3 e F2 juntos.

### F4 — Acesso por ponto `o.to` não é rastreado (BAIXA, pré-existente)

`/tmp/rev/d/k_campo_ponto.ath`: `for o in ops { pay(bank, o.to, o.memo) }`. O checker aceita, e a
análise diz `dinâmica` ("um valor calculado"), antes e depois. O caminho é sólido para tipos de uma
só variante, `Step::Field(variante, campo)`, mas exige informação de tipo na análise.

### Observações menores (sem reprodução de falha)

- `Obligations::discharged` usa `any` sobre os sítios com o mesmo span. Hoje cada chamada é
  visitada uma única vez, então não há conflito. É frágil se a análise algum dia visitar um corpo
  duas vezes (por exemplo, em ponto fixo de laço). `all` seria o seguro.
- No ARX e no Python, `false` com `linear` (`s.authorizes`) cai em R0632 antes do teste de
  `discharged`. Se `φ = s.authorizes(…) && X` e `X` falha num sítio descarregado, um defeito da
  análise aparece como R0632, e não como R0631. Os dois runtimes fazem igual.
- A obrigação levantada vale para **todos** os elementos da variante no caminho, inclusive os que o
  programa não passaria à ferramenta (chamada dentro de um `if`). É mais estrito que o monitor, mas
  é desenho; não gera R0631.

---

## O que se sustentou

**B1, identidade de elemento no mesmo `refine`.** Tudo `dinâmica` depois do reparo. Antes, tudo
era `descarregada` falsa.
- `x_lista_cruzada`: `[a, b]` iterada duas vezes;
- `x_ctor_cruzado`: `Pay { to: a.to, memo: b.memo }` reconstruído;
- `x_if_campos`: `if` por campo, mesmo span;
- `x_aninhado_cruzado` e `x_aninhado_mesma_linha`: listas aninhadas;
- `x_lista_de_listas`: `for l in [ops, ops]`;
- `x_alias_let_laco`: `let x = a` / `let y = b`;
- `x_bloco_sombra`: sombreamento em bloco.

Todos estão em `/tmp/rev/d/`.

**Controles descarregados e corretos nos dois runtimes.** Com plano válido, os pagamentos saem.
Com um elemento inválido, há `obligation_reject` no `refine` e nenhum R0631.
- `k_alias_lista`, `k_aninhado` (`rows[*].ops[*]`);
- `k_reconstroi` (`Pay { to, memo }` refeito do mesmo elemento);
- `k_lista_um` (`for x in [o]`);
- `k_match_duas_vezes`, `k_sombra_laco`;
- `k_troca` (`Pay { to: memo, memo: to }`, ligações trocadas, conferidas com mundo assimétrico);
- `k_if_campos_mesmo`, `k_else_outro_refine`.

**B2, `keeping … as`.** Tudo em `/tmp/rev/e/`.
- `as` em bloco aninhado (`b2_bloco_dentro`) e em braço de `match` (`b2_braco`): `dinâmica` →
  R0630. Antes: R0631.
- Uso depois do bloco (`b2_bloco_depois`): a lista externa endossada, descarregada e correta.
- Sombreamento por `for` (`b2_for_sombra`): `dinâmica`.
- `as` fora de `let` direto (`b2_cauda_bloco`, `b2_if`): o checker rejeita com E0438.
- A condição de ligação é a mesma na análise, no checker, no ARX (`block_inner`) e no emissor
  Python: só `let x = refine … as n`.

**Outras construções conferidas no código.** `else` do `refine` com literal → `dinâmica`. `on
failure(e)`/`on compensation_failed(cf)` ligam `Unknown`. Os tratadores `handle` começam com env
vazio, o que é conservador. Chamadas dentro de `Quant`/`Await` não registram sítio, logo
`discharged=false`, o que é seguro. Cada `for` tem span próprio, criado só no parser.

**(3) Corpus.** Rodei `regressao_corpus.py --antes /tmp/athc-antes --depois target/release/athc --out /tmp/rev/regressao_corpus.json`
(sem sobrescrever o do repositório): 210 programas, 17 com sítios.
- Só mudaram os 3 casos de reprodução: `b1_cruzado`, `b2_as_recusado_valido`, `b2_as_sombreia`.
- Sítios descarregados: 27 → 24. Dinâmicos: 0 → 3.

Também comparei o Python emitido (`emit --target py`) de todo o corpus, antes e depois. Os
programas aceitos dão saída **byte a byte idêntica**, exceto os 3 casos. Neles, a diferença é a
esperada: some `lifted=[…]` do `refine` e `discharged=True` da chamada. Os programas que os dois
compiladores rejeitam continuam rejeitados.

**(4) Monitores no efeito.** O diff só toca `obligations.rs`. Os monitores `requires` continuam no
ARX (`call_tool_live`, R0630/R0631/R0632) e no Python (`call_tool`, mesma ordem de teste). Em todos
os casos acima os dois runtimes concordaram em chamadas e código de aborto. A única exceção é
**F6/F1**, onde a divergência é pré-existente e vem da conferência levantada, não do monitor.

`bench/obrigacoes/relatorio.py --athc target/release/athc --out /tmp/rev/x.json`: a saída é
idêntica a `bench/obrigacoes/depois.json`, todos os runtimes concordam, os journals são idênticos
e não há R0631.

**Testes.** `cargo build --release -p athc` OK. `cargo test --all` (debug) e
`cargo test --all --release`: 319 passaram, 0 falharam, 1 ignorado, nos dois perfis. Isso inclui
`crates/athc/tests/ciclo1_obrigacoes.rs`.

## Sugestões de testes de regressão a acrescentar

- `f2_lit_if`, `f5_lit_match` e `f6_parcial`: `requires` com `if`/`match`/`all` não pode dar
  `descarregada` com literal; ARX e Python devem concordar.
- `a2_dois_refines_registro`: junção cruzada de dois `refine` deve ser `dinâmica`.
- `k_if_mesmo_laco` e `k_lista_dois` como controles de precisão, se F3 for tratado.

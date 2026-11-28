# Revisão adversarial final da análise de obrigações (HEAD `750a81d`)

Alvo: `crates/ath-syntax/src/obligations.rs` na branch `consolidacao/ciclo-1`, HEAD `750a81d`. O foco
foi o que o último commit mudou:
- o `join` compara os conjuntos de origens dos ramos;
- `refine … else` passa a ser tratado como junção;
- G1: um parâmetro ligado a dois campos sai como dinâmica.

Também reconferi as premissas de solidez.

Tudo foi feito localmente. Não usei API paga nem `--real`, não disparei workflow e não fiz commit nem
push. O worktree do usuário não foi tocado. A correção candidata foi compilada numa cópia por
`git archive` em `/tmp/wt3`, com `CARGO_TARGET_DIR=/tmp/wt3-target`. Os meus arquivos estão em `/tmp/rev3/`.

## Verificações pedidas

| verificação | resultado |
|---|---|
| `cargo build --release -p athc` | ok (o binário já estava atualizado com o HEAD) |
| `cargo test --all` | **exit 0**: 325 aprovados, 0 falhas, 1 ignorado, 43 suítes. `ciclo1_obrigacoes`: 10/10 |
| `varredura.py --athc target/release/athc` | 1.872 execuções, **0 R0631, 0 divergências**. Saída idêntica a `bench/obrigacoes/varredura_depois.json` |
| `regressao_corpus.py --antes /tmp/athc-antes --depois target/release/athc` | 297 programas, 99 com sítios. Descarregados 102 → 59; dinâmicos 7 → 50. Só mudaram casos de `bench/obrigacoes/`. JSON **idêntico** ao `regressao_corpus.json` versionado |
| programas reais (fora de `bench/obrigacoes`) | 10 programas com sítios, todos `descarregada`, nenhum `dinâmica` |

---

## Achados

### H1: um `requires` que não lê parâmetro nenhum sai "descarregada" sem conferência alguma (ALTA na classificação, mas o monitor ainda bloqueia; já existia antes do reparo)

**Causa.**
- Em `site()`, `params` guarda só os parâmetros que `φ` lê.
- Se `φ` não lê nenhum (por exemplo, só lê a fotografia `s` com literais), `per` fica vazio.
- O sítio então sai `Discharged { refines: [] }`, sem obrigação levantada para `refine` algum.

O F1 tinha tratado o caso de `params` vazio **por uma `free_vars` incompleta**, mas não o caso em
que `φ` realmente não lê parâmetros. A premissa "`φ` só lê os parâmetros e `s`" vale. O que falta é
quem confira `φ`: nenhum valor endossado leva a obrigação.

**Reprodução mínima** (`/tmp/rev3/z/z03_sem_params_refine.ath`; mundo `mundo_fechado.json` com
`flags=[maintenance]`; roteiro `rot_z3.json` com um `Pay alice/100` válido):

```
tool pay(c: Cap[Bank.Pay], to: Text ^ Endorsed, memo: Text ^ Endorsed) -> Unit ! { Bank.Pay } reversible latency 1.0
  requires s.member("flags", "payments_open")
…
    let p = refine (infer planner -> Plan { request = req }) by P else { return "none" }
    let ops = match p { Plan { ops } => ops }
    for o in ops { match o { Pay { to, memo } => pay(bank, to, memo) } }
```

- Esperado: `dinâmica`, e R0630 no efeito.
- Obtido: `L12 pay  descarregada  levantada para o refine em` (lista vazia). O ARX e o Python abortam
  os dois com **R0631** "defeito da análise".
- O mesmo acontece sem `refine` nenhum, com argumento literal (`z01_sem_params.ath`:
  `ping(bank, "alice")` com `requires s.member("payees", "eve")`).
- `/tmp/athc-antes` dá o mesmo resultado: o defeito já existia antes do reparo.

**Impacto.** É uma afirmação falsa de "descarregada" num programa aceito. O efeito não sai, porque o
monitor continua lá. Mas o código do erro (R0631) culpa a análise, e o relatório do compilador
afirma uma prova que não existe. O corpus não tem um `requires` assim. O padrão, porém, é realista:
uma chave global de liberação, uma janela de manutenção ou `s.is_managed("prod")`.

**Correção candidata** (`/tmp/rev3/fix_z1.patch`, 5 linhas, logo depois do cálculo de `params`):

```rust
if params.is_empty() {
    return Status::Dynamic { reason: "o contrato não lê parâmetro algum: nenhum `refine` o confere".into() };
}
```

**Validação da correção** (binário `/tmp/wt3-target/release/athc`):
- `z01` e `z03` passam a `dinâmica`, com R0630 nos dois runtimes.
- O corpus não muda: `regressao_corpus.py`, HEAD → correção, 0 programas mudaram (59/50).
- A varredura dá 1.872 execuções, 0/0, e a saída é **idêntica** à do HEAD.
- `ciclo1_obrigacoes` passa 10/10 e `ath-syntax` passa 5/5.
- O caso vazio legítimo (`for x in [] { t(x) }`) não é afetado: lá `params` não é vazio.

Sugiro um teste de regressão com `z03`.

### H2: o levantamento não olha o caminho; a obrigação de um ramo guardado vale para todo elemento e recusa planos válidos (MÉDIA, de comportamento, não de segurança; já existia antes)

Não é um falso "descarregada": não há R0631. É um excesso de conservadorismo que **muda o que o
programa faz**:
- A obrigação de um sítio é levantada para **todo** elemento da variante no caminho.
- Isso vale mesmo quando o sítio está atrás de um `if` que depende do elemento.
- Um plano em que o elemento "ruim" nunca chegaria ao efeito é recusado inteiro no `refine`
  (`obligation_reject`), e a execução vai para o `else`.

**Reproduções** (`/tmp/rev3/g/`). Mundo: `pairs=[alice|alice, alice|100, bob|5]`, `payees=[alice, bob]`.

`g01_guarda_member`, roteiro `[alice/100, eve/1]`:
```
for o in ops { match o { Pay { to } => if to == "eve" { "skip" } else { one(bank, to) … } } }
```
- Com o levantamento: `descarregada` → `obligation_reject` → resultado `"none"`, **0 efeitos**.
- Controle `g01d` (o mesmo programa, com o sítio forçado a dinâmica): `one(alice)` e resultado `"ok"`.

`g02_dois_destinos`, roteiro `[alice/x, bob/5]`:
```
match o { Pay { to, memo } => if memo == "x" { one(bank, to) } else { pay(bank, to, memo) } }
```
- As duas obrigações (`one` e `pay`) são levantadas para todo `Pay`.
- `alice/x` não satisfaz a de `pay`, embora nunca chegue a `pay`. O plano é recusado: `"none"`.
- Controle `g02d`: `one(alice)`, `pay(bob,5)` e resultado `"ok"`.

`/tmp/athc-antes` faz o mesmo: o comportamento vem do desenho do M-I.

Os `match` por **variante** são tratados com precisão (`v04_campo_por_variante` levanta uma
obrigação por variante). O problema é só com guardas `if` e com braços que levam a ferramentas
diferentes.

**Sugestões**, em ordem de custo:
- (a) documentar que o levantamento é insensível ao caminho;
- (b) tratar como `dinâmica` um sítio sob um `if` cuja condição lê algo ligado ao elemento;
- (c) levantar `cond ⇒ φ`, guardando a condição do caminho junto com a obrigação.

### H3: excesso de conservadorismo em código realista (BAIXA; já existia, em parte conhecido)

Todos estes casos dão `dinâmica` sem necessidade. O monitor confere no efeito, e não há falha de
segurança.

| caso | padrão | motivo |
|---|---|---|
| `z/z04_registro_inteiro` | `send(bank, o)` com `requires match o { Pay { to } => s.member("payees", to) }` | "`o` é o valor endossado inteiro" (o caminho termina em `Elem`) |
| `z/z05_campo_ponto` | `one(bank, o.to)` | `Field` é "um valor calculado" (F4, já conhecido) |
| `c/v02_juncao_variantes` | `let t = match o { Pay { to } => to, Gift { to } => to }; one(bank, t)` | variantes diferentes no mesmo span |
| `c/n05_else_lit_de_a` | `refine … else { match a { Pay { to } => Pay { to: to, memo: to } } }` | o `Lit` não é marcado, e o último id fica `None` contra `Rb` (forma do G4) |
| `c/a02_mesmo_conj_troca` | `z = if c { f } else { g }`, que é sempre `x` | últimos ids misturados vindos do `Lit` |

O G4 e o G5 da revisão anterior continuam como foram descritos.

---

## O que se sustentou (foco do último commit)

Nenhum falso "descarregada" e nenhuma divergência entre runtimes em nenhum dos casos abaixo. Cada
caso rodou nos dois runtimes, com `c` verdadeiro e falso, e com roteiros em que o cruzamento de
elementos ou de campos gera pares inválidos (`alice|5`, `bob|100`, `100|alice`). São 228 execuções
em `/tmp/rev3/c` (saída em `c_run.txt`), mais 16 em `/tmp/rev3/g`.

**`join` por conjunto, com correlações trocadas.**

| caso | ataque | resultado |
|---|---|---|
| `a01`, `m01` | `if c { Pair{a:x,b:y} } else { Pair{a:y,b:x} }`, campos `a` e `b` extraídos (conjuntos iguais, valores trocados) | `dinâmica` |
| `a02`, `a21`, `m09` | junção "igual" (sem marca) de dois valores com o mesmo conjunto, mas diferentes | `dinâmica` |
| `a03`, `m02` | o mesmo, mais uma junção que marca depois | `descarregada` e sólido |
| `a04`, `m03`, `m03b` | mistura com o valor anterior à marca | `dinâmica` |
| `a05` | junções iguais aninhadas (`w = if c {v} else {v}`, `u = if c {w} else {v}`) | `descarregada` e sólido (`u = v`) |
| `a06`, `a07`, `m04`, `m05` | `v = if c {x} else {y}`, `w = if c {y} else {x}`, `u = if c {v} else {w}` | `u` com `u` descarrega; `u` com `v` e `v` com `w` dão `dinâmica` |
| `a18`, `a14` | `if c { t } else { t }` e `if c { o } else { o }` | `descarregada` e sólido |
| `a16`, `a17`, `m10`, `m11` | `if` dentro de lista literal, e listas literais cruzadas | sólidos |
| `a19` | sombreamento num bloco com junção | `dinâmica` |
| `a20` | `rows` contra `ops` | `dinâmica` |

**G1, um parâmetro com dois campos.**
- `a10`/`a11` (reconstrução com campos trocados num `if`) e `a12`/`a12b` (lista literal de
  reconstruções) dão "campos diferentes" → `dinâmica`.
- `a09` (literais iguais) descarrega e é sólido.

**Variantes com os mesmos campos.**
- `v01`, `v04` e `v06` levantam uma obrigação por variante.
- `v05` troca variante e campo por `Lit` (`Pay{to,memo} => Gift{to: memo, …}`). A obrigação é
  levantada como "Pay: `memo` ∈ payees" e "Gift: `to` ∈ payees", o que é correto.
- Os roteiros inválidos são recusados no `refine` e os válidos executam.

**`refine … else` como junção.**

| caso | `else` | resultado |
|---|---|---|
| `n01` | literal | `dinâmica` |
| `n04` | `refine` aninhado | descarrega e é sólido; três `refine` levantados |
| `n06` | mistura `b` com `a` | `dinâmica` |
| `n07` | `if` trocado em dois `refine` | `dinâmica` |
| `n08` | dois `refine` com `else { a }` | `dinâmica` |
| `n09` | `y` com `else { x }` e `x` com `else { a }` | `y` com `y` é sólido; `y` com `x` dá `dinâmica` |
| `p02` | plano vazio | descarrega; o `else` não gera chamadas |
| `p03` | `Plan { ops: ops1 }` de um `refine` anterior | descarrega, levantada para os dois |
| `p04` | cruzado | `dinâmica` |
| `p05` | `Op` literal | `dinâmica` |

**Fuzz** (`/tmp/rev3/fuzz.py`, sementes 11–14).
- Programas aleatórios com junções `if`, `Pair` trocado, reconstrução, troca de campos, blocos,
  `if` com `return`, laços sobre listas literais e `refine … else` com fallback, em dois cabeçalhos
  (laço duplo sobre um `refine`, e dois `refine` de `Op`).
- Foram 280 programas e 3.216 execuções. 266 programas têm sítio descarregado; **68 sítios
  "cruzados" com variáveis distintas foram descarregados**.
- Resultado: **0 R0631 e 0 divergências**.
- Prova de que o fuzz acha defeitos: com `/tmp/athc-antes`, os primeiros 25 programas da semente
  11 dão **24 R0631 em 300 execuções**.

**Por que a regra do conjunto é sólida.**
- Uma origem `Refine` só recebe o último id K em K.
- Depois de K, só passos `Field` preservam a origem; `Elem` e uma junção que marca empilham um id
  novo.
- `Lit` e `List` não propagam marcas para dentro. Isso deixa as fontes internas com os seus últimos
  ids, e a mistura cai na regra do último id.
- Portanto, (span, caminho, último id) designa um único valor por escopo.
- Dois ramos com o mesmo conjunto designam os mesmos candidatos. Não marcar não cria uma
  correlação nova: cada parâmetro continua designado por uma origem do conjunto, e a regra do
  último id rejeita conjuntos com ids diferentes.
- A comparação por janelas é transitiva, e a ordem só pode tornar a análise mais conservadora (a
  igualdade de `Lit` depende da ordem).
- Um ramo vazio é sempre inalcançável ou divergente: `return`, `Lit` de outra variante ou `Elem` de
  lista vazia. Por isso filtrá-lo é sólido.

## Premissas reconferidas

- **Sem closures, funções nem mutação.**
  - `Stmt` tem só `Let` e `Expr`.
  - `Item` não tem funções, e `Call` a algo que não é ferramenta dá `Unknown`.
  - Não há atribuição.
- **Laços.** `for` devolve `Unknown`, e as ligações do corpo são truncadas. `Quant` e `Await` não
  são percorridos: uma chamada ali não vira sítio, e portanto nunca é "descarregada" (conservador).
  Os compensadores e `reconcile` chamam com `call_site = None`.
- **Sítios únicos.** Nenhuma expressão é analisada duas vezes. Os spans de `if`, `match`, `refine` e
  da variável do `for` vêm de construtores distintos do parser (`parser.rs:1175/1372/1444/1517`).
- **Fotografia `s`.** É capturada uma vez nos dois runtimes (`runtime.py:189`; no ARX, os métodos
  `s.*` leem a fotografia imutável). `s.authorizes` é linear e dá R0632, não R0631.
- **`keeping … as`.** A ligação só ocorre quando o `refine` é o valor do `let`, igual no checker, no
  ARX, no emissor e no codex.
- Os runtimes concordaram em todas as execuções.

## Fora do escopo ou não exercitado

- A retomada por fita (WAL/tape).
- Concessões `s.authorizes` com o mesmo elemento em dois sítios. Isso cai no H2 e daria R0632, não
  R0631.
- Programas com vários agentes.

## Artefatos

| artefato | caminho |
|---|---|
| Casos | `/tmp/rev3/cases.py`, `/tmp/rev3/cases_g.py`, gerador `/tmp/rev3/gen.py`, programas em `/tmp/rev3/c/`, `/tmp/rev3/g/`, `/tmp/rev3/z/` |
| Harness | `/tmp/rev3/h.py`; saídas `/tmp/rev3/c_run.txt`, `/tmp/rev3/c/resultado.json`, `/tmp/rev3/g/resultado.json` |
| Fuzz | `/tmp/rev3/fuzz.py`; saídas `/tmp/rev3/fz11.txt`…`fz14.txt`; programas em `/tmp/rev3/fz1{1..4}/`; controle `/tmp/rev3/fzA/` |
| Correção | `/tmp/rev3/fix_z1.patch`; cópia `/tmp/wt3`; binário `/tmp/wt3-target/release/athc` |
| Validação da correção | `/tmp/rev3/rc_fix.json`, `/tmp/rev3/varredura_fix.json`, `/tmp/rev3/wt3_test.txt` |
| Verificações do HEAD | `/tmp/rev3_cargo_test.txt`, `/tmp/rev3_varredura.json`, `/tmp/rev3_rc.json` |

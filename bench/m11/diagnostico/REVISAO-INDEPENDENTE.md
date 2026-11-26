# Revisão adversarial — renomeação de campo em padrão, E0422, palavra reservada

Árvore: `/home/claude/aletheia`, branch `consolidacao/ciclo-1`, mudanças não commitadas (8 arquivos + 2 testes novos).
Compilador comparado: `/tmp/frozen-target/release/athc` (223fb53). Tudo local: nenhuma chamada a API paga, nada com `--real`, nenhum workflow, nenhum commit.
Programas de teste: `/tmp/rv/*.ath`; script dos dois runtimes: `/tmp/rvtools/both.sh` (check + `athc run` com scripted + `emit --target py` + `aletheia_runtime run`).

`cargo build --release -p athc`: ok. `cargo test --all`: exit 0, 42 suítes, 314 testes passaram, 0 falhas.

## Resumo

Nenhuma garantia de segurança (integridade, confidencialidade, pc, origem, limites, E0451, E0421/E0422, `keeping`, obrigações levantadas, concordância ARX/Python) cai por causa da renomeação. Há um defeito real introduzido pela mudança (A1, o Codex deixa de ser invariante a alfa-renomeação), dois defeitos pequenos introduzidos (A2, A3) e dois defeitos anteriores na análise de obrigações (B1, B2). Esses dois vêm de antes, o compilador congelado também os tem, e o monitor do efeito os fecha em runtime.

---

## Achados introduzidos pela mudança

### A1 — Codex: o nome local renomeado entra na forma canônica (perde a invariância a alfa)
**Severidade: média (canonicalização/endereçamento; não é furo de segurança). Defeito real.**

O Codex usa índices de De Bruijn para os locais: `let`, `for`, quantificadores e `on failure(e)` não imprimem o nome. Verifiquei: `let k = item … k` e `let j = item … j` dão a mesma raiz, `#1d9b32454216`. O cabeçalho novo do braço imprime `campo:nome`, então o nome local do padrão vai parar no hash.

Reprodução (`/tmp/rv/alpha.ath`, política `Q` com um braço trocado):

| braço | raiz |
|---|---|
| `Use { item: k } => s.member("items", k)` | `#47a1193eee1b` |
| `Use { item: j } => s.member("items", j)` | `#d9744dfaa36c` |
| `Use { item } => s.member("items", item)` | `#7f32b262be9c` |
| `Use { item: item } => s.member("items", item)` | `#7f32b262be9c` |

- **Esperado:** os quatro são alfa-equivalentes e deveriam ter a mesma raiz, como acontece com `let`.
- **Obtido:** três raízes diferentes.

Não encontrei colisão: o cabeçalho junto com os índices de De Bruijn distingue programas que de fato são diferentes. O dano fica em hashes de contrato e política, que mudam por uma troca de nome cosmética.

**Correção sugerida** em `crates/ath-codex/src/lib.rs`, perto da linha 704: imprimir só `a.fields` (os nomes dos campos), nunca `b.name`. Os índices de De Bruijn já ligam cada uso à posição do campo. Na forma abreviada `fields == binds`, então as raízes antigas continuam idênticas. Conferi `Use { a: x, b: y } => x` contra `Use { a: y, b: x } => x`: os campos ficam `a b` e o corpo vira `#1` contra `#0`, ainda distintos.

### A2 — `campo: _` não confere se o campo existe nem se repete
**Severidade: baixa (diagnóstico). Defeito real.**

O parser descarta o par inteiro quando o nome é `_`, antes de o checker ver o campo.
- `Use { iten: _ } => true` (campo com erro de digitação): aceito. `Use { iten } => true` dá E0010.
- `Use { item: _, item: _ } => true`: aceito, sem E0014.

Isso não liga nada, então não há impacto de segurança. Porém um `campo: _` com o nome errado passa calado. Correção: guardar o campo também quando o nome é `_` (numa lista separada ou com um marcador) e passar por E0010/E0014.

### A3 — A mensagem nova de palavra reservada também aparece quando o nome simplesmente falta
**Severidade: baixa (qualidade de diagnóstico). Defeito real, mas cosmético.**

`Parser::ident()` transforma qualquer palavra reservada encontrada onde se espera um nome em "não pode ser usada como nome". Quando o autor esqueceu o nome, a orientação engana:
- `let p = refine d by else { … }` dá "`else` é palavra reservada e não pode ser usada como nome: escolha outro (por exemplo `else_value`)". O problema real é que falta a política.
- `type Plan = |` seguido de `model planner { … }` dá "`model` é palavra reservada… (por exemplo `model_value`)". O problema real é que falta a variante.

Sugestão: só dar a mensagem de "usar como nome" quando o próximo token mostra uso como nome (`:`, `=`, `,`, `}`, `(` depois da palavra). Nos outros casos: "esperado um nome, encontrado a palavra reservada `else`".

(Detalhe: `Use { .., item: k, .. }`, com dois `..` e `..` fora do fim, é aceito. O laço do parser já tratava `..` assim antes.)

---

## Achados anteriores (o compilador congelado 223fb53 também tem; não vêm da renomeação)

### B1 — Obrigações: argumentos de elementos diferentes da mesma lista contam como "o mesmo elemento"
**Severidade: média (a afirmação estática "descarregada" é falsa; o monitor do efeito fecha em runtime). Defeito real, anterior.**

`Step::Elem` não distingue iterações. Dois `for` aninhados sobre a mesma lista endossada produzem o mesmo caminho `[Plan.ops, Elem]`. `site()` então levanta a obrigação para o `refine` e marca o sítio como `Discharged`. Isso é falso quando `requires` relaciona os parâmetros, como em `s.binds`/`s.authorizes`.

`/tmp/rv/cross.ath` (mundo `wc.json`, modelos `mc.json`):
```
tool pay(c: Cap[Bank.Pay], to: Text[..16] ^ Endorsed, memo: Text[..16] ^ Endorsed) -> Unit ! { Bank.Pay } reversible latency 1.0
  requires s.binds("pairs", to, memo)
...
for o in ops { for o2 in ops { match o { Pay { to: a } => match o2 { Pay { memo: b } => pay(bank, a, b) } } } }
```
Com `pairs = ["alice|100","bob|5"]` e o plano `[alice/100, bob/5]`, cada elemento passa na obrigação levantada no `refine`, então o plano é aceito. O check marca o sítio como descarregado (`discharged=True` no Python emitido).

- **ARX e Python:** o primeiro `pay(alice,100)` executa. Depois `pay(alice,5)` aborta com `R0631 obrigação de pay descarregada e violada: defeito da análise`.
- **Esperado:** o sítio é `Dynamic`, porque os argumentos vêm de elementos diferentes.

Nenhum efeito não autorizado executa. Mesmo resultado no congelado com a forma abreviada (`/tmp/rv/cross_old.ath`).

### B2 — Obrigações: `let x = refine … keeping … as nome` não liga `nome` no ambiente da análise
**Severidade: média (mesma classe que B1, fecha em runtime). Defeito real, anterior.**

`obligations.rs`, `Stmt::Let`, só empurra `name`. O `as nome` não entra no ambiente, então `lookup(nome)` resolve para uma ligação externa com o mesmo nome. O ARX e o checker ligam os dois nomes.

`/tmp/rv/asb.ath`: um `let fora` externo vem de um `refine` endossado. Depois vem `let q = refine d by P keeping ops by Q as fora`, e `for o in fora { … pay(bank, to, memo) }` usa a lista recusada, que é Untrusted. A ferramenta tem parâmetros sem rótulo e `requires s.member("payees", to)`.
- O check aceita e marca o sítio como descarregado no `refine` errado.
- **ARX e Python:** `R0631`.
- **Esperado:** o sítio é `Dynamic`, porque o valor vem do que a política recusou, não de um endosso.

Correção: em `Stmt::Let`, quando o valor é `Refine { keep: Some(Keep { bind: Some(b), .. }) }`, empurrar `(b, [Src::Unknown("o que o refine deixou de fora")])`.

---

## O que tentei e resistiu

**Varredura de todos os pontos que percorrem `match`/`arm.binds`.**
- Usam o campo real: `check.rs` (match_expr), `obligations.rs`, `ath-emit`, `arx`, `ath-codex`.
- Usam o nome local e estão corretos para sombreamento: `linear.rs` (linhas 76 e 193, "o braço liga x, a contagem é zero"), a lista de locais do codex.
- Só percorrem os corpos dos braços, sem nomes: `budget.rs` (268, 350), `linear.rs` 325, `ast.rs` walk_mut, `expr_any`, codex 240.
- Não há outro construtor de `Arm` além do parser. O curinga expandido recebe `fields: Vec::new()`. `ath-grammar` e `ath-world` não tocam em braços.
- O runtime Python não tem caminho próprio de `match`: só `R.bind(valor, campo)` emitido, mais `lifted` (param ← campo), que vem da análise.

**Rótulos.**
- O nome local herda `s.lab`, `s.conf`, `refn` do campo real e `s.origin` (`check.rs:3172-3177`).
- Os pares Untrusted/Secret dos testes novos conferem.
- Rótulo em campo de dado é recusado (E0004), então não há rótulo por campo a perder.

**Troca de campos `Pay { to: memo, memo: to } => pay(bank, to, memo)`** (`/tmp/rv/swap.ath`). A política aprova `to = alice`, a chamada paga `memo = "mallory"`.
- A obrigação levantada sai como `[(1, "memo")]`.
- ARX e Python recusam no `refine` ("viola o contrato de `pay`") e o resultado é `"none"`.
- O controle com nomes corretos (`Pay { to: x, memo: to } => pay(bank, x, to)`) é aceito.
- No `treasury.ath` com `Pay { to: amount, amount: to, memo }`, o `lifted` mostra `[(2, "to"), (1, "amount")]`, a troca certa.

**Limites e E0451.** Em `Use { big: small, small: big } => small * 2 < 10`, com `big: Int` sem limite, dá E0451. Com `big * 2`, que é o campo `Int[0..9]`, passa. O limite segue o campo, não o nome.

**Ancoragem E0421.**
- `let items = s.inventory("items")`, depois `match … { Rec { item: items } => infer … { items = items } }`: dá E0421. Correto, porque o nome sombreado deixa de ser âncora.
- Com `Rec { item: zz }`, o `items` externo continua âncora e passa.
- Conferi que `let_anchor` não deixa entradas velhas na profundidade do escopo do braço.

**Sombreamento.**
- Nome do braço igual a um parâmetro da política (`Note { body: o }`), igual ao parâmetro `aceitos`, ou trocando `item`/`units`, em `match` aninhado sobre `aceitos` (`/tmp/rv/keep.ath`): ARX e Python dão o mesmo resultado, "2 mantidas, 3 unidades, 2 fora", conferido à mão.
- O Python usa nomes frescos (`v_x_N`): não há colisão com o valor examinado nem com palavras-chave do Python.

**`s` como nome local** (`Use { item: s } => s.member("items", s)`). `s.` continua intrínseco no checker, no ARX, no emissor e nos dois runtimes, que concordam ("4 mantidas, 48 unidades"). Um `let s = …` já era aceito no congelado, então não é classe nova.

**Campo ausente no valor.** O ARX (`filter_map`) ligaria silenciosamente um nome externo, e o Python daria R0100. Esse caminho é inalcançável: a saída do modelo sem o campo é descartada pela forma ("fora do tipo `Plan`") nos dois runtimes, um construtor exige todos os campos, e chaves e capacidades não podem ser guardadas em dados (E0004), o que exclui uso duplo linear via campo.

**Raízes Codex de programas antigos.** 202 arquivos `.ath` em `examples/` e `bench/`. Os que compilam nos dois compiladores têm saída idêntica do `check`, inclusive a raiz. Nenhum programa antes aceito passou a ser recusado. Passaram a ser aceitos só `bench/m11/diagnostico/exemplos/renomear.ath` e `bench/m11/runs/authoring/shipping/ath/rep1/v{1,2}.ath`. As outras 43 diferenças são mudanças de texto de diagnóstico em programas que já eram recusados.

**E0422.** A condição é a mesma de antes (`any`, agora `find_map`) e o estrato continua E3. O diagnóstico aponta a comparação, a política entra como rótulo secundário, e a orientação cita `s.inventory("k")`/`s.member("k", …)`. Só cita `s.is_managed` quando o prompt recebe `managed`.

**Sintaxe.** `campo = nome` é recusado e indica `:`. `_: x` é recusado. `item: k: j` é recusado. `_ { x }` é recusado no curinga. Vírgula no fim é aceita.

## Notas para quem vier depois
- Os testes do runtime precisam de `"sinks": ["tool"]` no mundo simulado para que uma ferramenta arbitrária execute. Sem isso o resultado é R0600 depois do `requires`.

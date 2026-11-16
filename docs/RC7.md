# Aletheia v1.0-RC7: execução parcial sob política

Na RC6, com um modelo real, a injeção de "falso fim dos dados" fez a política recusar os três
rascunhos de todas as dez execuções: a segurança se manteve e a tarefa foi negada inteira, por
causa de uma nota. A RC7 dá à linguagem a forma de executar a parte segura de um plano e deixar
o resto de fora, registrado:

```
let plan = refine draft by SafePlan keeping notes by SafeNote retry 3 else { … }
```

```sh
scripts/rc7-smoke.sh                  # a demonstração, sem rede (exit 0 = todas as conferências)
cargo test -p athc --test rc7         # cinco testes
cd lean && lake build --wfail         # Prune.lean e as 167 guardas de axiomas
```

As RC1 a RC6 continuam passando sem alteração, e as raízes do Codex dos programas anteriores não
mudaram (`#4877f356ba52`, `#641754c91ccf`, `#c20feae41e92` na triagem da RC6).

## 1. O defeito, reproduzido antes de corrigir

O commit `RC7: a negação de serviço por injeção como teste que falha` só acrescenta o teste (e a
face `/gh/issues` do `ath-world`, com o formato da API de issues do GitHub, para testar sem
rede). O teste falha nele: `docs/evidence/rc7-negacao-na-rc6.txt`.

Antes de mudar a linguagem, conferi se o defeito era do programa ou da linguagem. Era da
linguagem. A alternativa manual (enfraquecer a política do plano e filtrar com
`if SafeNote(n) { commit comment(…) }` dentro do laço) compila, mas a obrigação levantada do
contrato de `comment` (M-I, Rev13) é conferida no `refine` sobre o plano inteiro, sem olhar o
`if`, e recusa os três rascunhos do mesmo jeito. E decidir pela fotografia confiável fora de uma
política (`if s.member(…)`) é `E0423`. Na RC6 não havia programa que executasse só a parte
segura de um plano.

## 2. `keeping`

`refine d by P keeping campo by Q retry n else { … }`, para cada amostra:

1. a amostra é decodificada no tipo do `infer` (como antes);
2. da lista `campo` do plano, os elementos que `Q` recusa são deixados de fora, na ordem; o que
   saiu vai para o journal (`prune`: a política, o campo, quantos ficaram, os valores que saíram);
3. se a lista ficou abaixo do mínimo do tipo, a amostra é recusada (`prune_underflow`): o valor
   não sai do tipo;
4. **só então** `P`, as obrigações levantadas dos contratos e o endosso veem o plano: o que
   restou é o que será executado, e é ele que tem de satisfazer tudo;
5. recusado, nova amostra; esgotadas, o `else`.

Conferido por `athc check`: o campo existe no tipo do rascunho e é uma lista, e `Q` recebe um
elemento dela (`E0437`, `E0450`); o inventário que `Q` lê está no prompt (`E0421`, `E0422`, como
para `P`). O Codex inclui a cláusula só quando ela existe. ARX e Python implementam o mesmo
algoritmo, com o mesmo journal byte a byte (`keeping_is_the_same_in_both_runtimes`). A retomada
reproduz a poda: as políticas são puras e a fotografia é a mesma (`R0652`).

A saída diz o que aconteceu: `✓ Endorsed(SafePlan) · 1 deixado(s) de fora por SafeNote`.

## 3. Em Lean: `Prune.lean`

Seis teoremas, 167 guardas `#print axioms` no total (eram 161), sem `sorry`.

| Teorema | Formulação |
|---|---|
| `keeping_sound` | o plano devolvido satisfaz `P`, todo elemento da lista satisfaz `Q`, a lista é uma subsequência da proposta (nada inventado, mesma ordem), o resto do plano é o proposto, o mínimo é respeitado |
| `keeping_keeps_every_accepted` | a lista devolvida é exatamente o filtro por `Q`: nada que `Q` aceita é perdido, e o que saiu foi recusado por `Q` |
| `keeping_extends_strict` | um plano que a RC6 aceitava (e cujos elementos `Q` aceita) é aceito igual |
| `keeping_live` | se o que resta da poda é um plano que `P` aceita, ele é devolvido |
| `strict_denies` | o defeito da RC6: um elemento fora do cadastro faz o `refine` estrito recusar um plano cujo resto a mesma política aceita |
| `checking_before_pruning_is_unsound` | decidir `P` antes da poda executaria um plano que `P` recusa (uma política que exige as duas notas, e a poda deixa uma) |

O modelo é da decisão do `refine`; não está no núcleo λ_Ath (`Eval.lean`) nem no tripé.

## 4. Medido

**Sem rede** (`tests/rc7.rs`, `scripts/rc7-smoke.sh`), com o plano que a injeção conseguiu
(notas para as duas issues do cadastro e uma para a canário): a RC6 não faz nada; a RC7 comenta
nas duas, nenhuma vez na canário, abre o resumo e registra a nota deixada de fora. Sem nada
seguro, a política do plano recusa o plano vazio e o `else` é tomado. Morto depois de enviar o
primeiro comentário, sob supervisão, termina sem duplicar.

`cargo test --all`: 213 aprovados, 0 falhas (207 da RC6, 5 de `tests/rc7.rs`, 1 diferencial).

**Com o modelo real** (`openai/gpt-oss-120b`, contra o GitHub, a injeção de "falso fim dos
dados", 10 execuções, 16/11/2026, US$ 0,0022):

| | RC6 | RC7 |
|---|---|---|
| o modelo propôs nota para a canário | 10/10 (a RC6 registrou 1/10: o detector perdia casos) | 10/10 |
| a política recusou o plano inteiro | 10/10 (três rascunhos cada) | 0/10 |
| `keeping` deixou a nota da canário de fora | — | 10/10 |
| comentário na canário | 0/10 | 0/10 |
| comentário na issue A | 0/10 | 10/10 |
| comentário na issue B | 0/10 | **0/10** |
| issue de resumo | 0/10 | 10/10 |
| título do resumo citando a canário (#279) como triada | — | **8/10** |

A injeção mandava escrever "a segunda nota" para a canário, e o modelo obedeceu: **trocou** a
nota da issue B pela da canário. A poda tirou a da canário, mas não tinha como devolver a de B,
que o modelo nunca escreveu. E o título e o resumo, escritos pelo modelo para o plano que ele
propôs, passaram a descrever um plano que não foi executado.

## 5. O que isso quer dizer

* **A segurança não mudou**: o que sai para o mundo é o que as duas políticas e os contratos
  aceitam (`keeping_sound`). Nenhuma nota chegou à canário, nem na RC6 nem na RC7.
* **A negação total acabou**, e a negação que sobra é a que o próprio modelo produz: um item que
  ele omite não pode ser recuperado por poda. Exigir na política do plano uma nota por issue do
  cadastro traria de volta a recusa do plano inteiro sempre que o modelo for capturado assim.
* **A execução parcial expõe um problema que a recusa total escondia: o conteúdo.** Os campos de
  texto do plano (título, resumo) foram escritos para o plano proposto, não para o executado.
  A política decide **quais** ações saem; o texto que elas levam é do modelo e pode citar o que
  foi deixado de fora. Na RC6 isso não aparecia porque nada saía.

## 6. Limites

* **`keeping` poda um campo, um nível.** Não há poda em listas aninhadas nem em mais de um campo
  no mesmo `refine`.
* **O programa não vê o que foi deixado de fora.** Fica no journal e na saída; o programa não
  tem como, por exemplo, escrever no resumo "1 nota recusada". Os elementos podados são dados
  do modelo, e dar acesso a eles exige decidir o rótulo de integridade (seriam `Untrusted`).
* **Os campos de texto do plano não são reconciliados com a poda** (seção 5). Uma política de
  plano que os conferisse precisaria de predicados sobre texto que a linguagem não tem.
* **A obrigação levantada continua de caminho insensível**: ela confere o plano que resta, mas
  ignora `if` dentro do laço (seção 1). Com `keeping`, isso deixou de impedir a execução parcial.
* **Uma injeção, um modelo, dez execuções** com o modelo real.
* **`Prune.lean` modela a decisão do `refine`**, não a execução do plano depois dela; a ligação
  com o executável é por correspondência de enunciados e pelos testes.

## 7. Códigos novos

| Código | Significado |
|---|---|
| `E0437` | `keeping` sobre campo que não existe ou não é lista, ou com política que não recebe o elemento |

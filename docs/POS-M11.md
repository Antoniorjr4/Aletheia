# Pós-M11: três correções e a reanálise dos arquivos (26/11/2026)

Versão posterior à congelada (`223fb53`). A medição do M11 continua presa a `223fb53`, e os
resultados dela não mudam: [bench/m11/AVALIACAO-LOCAL.md](../bench/m11/AVALIACAO-LOCAL.md). Tudo
aqui é **diagnóstico posterior às correções**: não é campanha nova, não é cega e não diz como o
modelo teria respondido às mensagens novas. Gasto adicional com API: **US$ 0,00**. A1 e A4 continuam
sem resultado.

## 1. Renomeação de campo em padrão

Forma canônica: `Variante { campo: nome }`. O campo real continua sendo `campo`; `nome` é a ligação
local, visível só no corpo do braço. A forma abreviada `Variante { campo }` não mudou, e as duas se
misturam no mesmo padrão:

```
policy Unico(o: Op, aceitos: List[Op, 0..8]) -> Bool = match o {
  Use { item: k } => (sum a in aceitos => match a { Use { item } => if item == k { 1 } else { 0 }, _ => 0 }) == 0,
  _ => true,
}
```

Regras:

- **O nome local recebe tudo o que o campo tem:** tipo, limites (`Int[a..b]`, inclusive para o
  E0451), integridade, confidencialidade e origem (a nota "origem:" dos diagnósticos continua
  apontando a ferramenta). As obrigações levantadas para ferramentas com `requires` seguem o campo
  real, não o nome local.
- **Campo inexistente:** E0010, apontando o campo.
- **Campo repetido** no mesmo padrão (`Use { item: a, item: b }`, `Use { item, item: b }`): E0014.
- **Nome ligado duas vezes** (`Use { a: x, b: x }`, `Use { units: item, item }`): E0014.
- **Escopo:** é o mesmo de antes. O nome vale só no braço e pode sombrear um nome de fora, como a
  forma abreviada já fazia. Um campo renomeado não fica visível pelo nome original.
- **Hash:** o nome local não entra no Codex. Como em `let` e `for`, ele é posição, então
  `Use { item: k }`, `Use { item: j }` e `Use { item }` têm a mesma raiz.
- `Use { item: _ }` ignora o campo, mas o campo tem de existir e não pode se repetir (E0010, E0014).
  `Use { item = k }` é recusado com
  "num padrão, o campo se renomeia com `:`: escreva `item: nome`".

Testes ([crates/athc/tests/ciclo1_renomear.rs](../crates/athc/tests/ciclo1_renomear.rs), 5 testes):

- Equivalência com o contorno por `let` e com a renomeação dos dois lados do `match` aninhado. Seis
  casos derivados à mão, comparados nos dois runtimes (ARX e Python emitido).
- Campo inexistente, campo repetido e nome ligado duas vezes.
- Os limites do campo passam para o nome local.
- Quatro pares abreviado × renomeado com `Untrusted` e `Secret`: os dois são recusados pelo mesmo
  código (E0412, E0440), com os mesmos diagnósticos. Há controles confiáveis aceitos.
- `examples/rc4/treasury.ath` com todos os campos renomeados levanta exatamente as mesmas obrigações.

Três mutações conferem que os testes pegam o erro:

- rótulos apagados na renomeação → falha o teste de `Untrusted`/`Secret`;
- linhagem pelo nome local → falha o teste das obrigações;
- interpretador buscando pelo nome local → falha a equivalência.

## 2. E0422 e palavras reservadas

**E0422.** A regra não mudou: com o prompt ancorado, a política não pode comparar texto com um
literal. Mudou o que o diagnóstico mostra:

- o span principal agora é a comparação responsável, e a cláusula que usa a política aparece como
  contexto;
- a nota lista as listas que o prompt deste programa recebe;
- a ajuda sugere `s.member`/`s.binds` com essas listas, ou apagar a comparação se a lista já garante
  a regra;
- `s.is_managed` só aparece se o programa usa `s.managed()`.

Testes em [crates/athc/tests/ciclo1_diagnosticos.rs](../crates/athc/tests/ciclo1_diagnosticos.rs).

```
error[E0422]: descompasso entre o inventário confiável e a política
 5 | … s.binds("next_stages", opp, stage) && stage != "closed_won" }
   |                                         ^^^^^^^^^^^^^^^^^^^^^ compara texto com um literal escrito no programa
12 |     let plan = refine d by P keeping ops by Ok as left …
   |                                             -- `Ok` decide este rascunho
   = o prompt recebe `s.inventory("next_stages")`; a política tem de decidir pelas mesmas listas, …
   = ajuda: decida pela lista confiável no lugar do literal: `s.member("next_stages", x)` ou …
```

**Palavra reservada como nome.** A mensagem agora é "esperado um nome, encontrado `authority`:
`authority` é palavra reservada e não pode ser usada como nome; se era o nome, escolha outro (por
exemplo `authority_value`)", com o span na palavra. A mesma frase serve quando o nome faltou antes
de uma palavra reservada (`refine d by else`), sem afirmar que ela era o nome. Vale para
`let`, parâmetros e campos de `infer` (o caso `policy = …` do M11). A gramática não mudou, e
`authority_cents` continua sendo um nome comum.

## Alterações

| onde | o quê |
| --- | --- |
| `ath-syntax` (AST, parser, lexer, obrigações) | `Arm.fields`; `campo: nome` no padrão; mensagem para `campo = nome`; `Tok::reserved_word`; linhagem pelo campo real |
| `ath-check` | nome local com o tipo, os limites e os rótulos do campo; E0014; E0422 com span e orientação do programa |
| `ath-emit` | `R.bind(valor, campo)` lê o campo real e liga ao nome local |
| `arx` (runtime Rust) | o escopo do braço busca o campo real |
| `ath-codex` | o cabeçalho do braço lista só os campos, e o nome local é posição; as raízes antigas não mudam |
| runtime Python | **nenhuma** |

Corpus versionado fora das autorias (130 programas, inclusive as autorias do M10): aceitação,
códigos de diagnóstico, raiz do Codex, tabela de objetos e Python emitido **idênticos** nos dois
compiladores. Rust: 315 testes passam (305 antes, mais 10 novos). Passam também os testes Python
do runtime, do AgentDojo, do M10 e do orçamento.

## Revisão independente

Um revisor que não escreveu a mudança atacou a renomeação sem gastar nada
([bench/m11/diagnostico/REVISAO-INDEPENDENTE.md](../bench/m11/diagnostico/REVISAO-INDEPENDENTE.md)).
Ele não achou caminho para apagar `Untrusted`/`Secret`, escapar de limites ou E0451, desviar
obrigações (troca `Pay { to: memo, memo: to }` levanta a obrigação certa e é recusada), nem
divergência entre ARX e Python.

Achados introduzidos pela mudança, **corrigidos neste lote**:

- **A1, médio:** o nome local entrava no hash do Codex. Agora o cabeçalho do braço lista só os
  campos; há teste.
- **A2, baixo:** `campo: _` não conferia o campo. Agora dá E0010/E0014; há testes.
- **A3, baixo:** a mensagem de palavra reservada afirmava uso como nome quando o nome tinha faltado.
  Agora vale para os dois casos; há teste.

Achados **anteriores** (o compilador congelado também os tem), **não corrigidos neste lote**:

- **B1:** dois `for` aninhados sobre a mesma lista endossada fazem a análise tratar argumentos de
  elementos diferentes como do mesmo elemento. O sítio sai "descarregado" quando deveria ser
  dinâmico. Os dois runtimes abortam com R0631 ("defeito da análise") na chamada que viola. Antes
  dela, uma chamada que satisfaz o contrato já executou.
- **B2:** em `let q = refine … keeping … as fora`, a análise de obrigações não liga `fora`. Um
  `fora` de fora do `let` é usado no lugar, e os dois runtimes abortam com R0631.

Nenhum dos dois executa efeito não autorizado: o monitor na fronteira do efeito fecha os dois casos.
Mesmo assim, a afirmação estática "descarregada" fica falsa. Ficam registrados como pendência,
fora do escopo desta entrega.

## 3. Reanálise das 71 versões Aletheia arquivadas

[bench/m11/diagnostico/reanalise_compilador.py](../bench/m11/diagnostico/reanalise_compilador.py).
Saída em [pos_correcoes/reanalise.md](../bench/m11/diagnostico/pos_correcoes/reanalise.md) e
`reanalise.json`. Nenhum programa foi editado. O Python fica como referência: suas primeiras
versões rodam em 13/16 e as finais em 15/16.

| versões | compilam no congelado | compilam na nova |
| --- | --- | --- |
| primeiras (16) | 0 | **1** (`shipping` rep1 v1) |
| intermediárias (39) | 0 | 1 (`shipping` rep1 v2) |
| finais (16) | 9 | 9 |
| todas (71) | 9 | 11 |

As 24 versões que paravam na renomeação com `:` agora passam desse ponto:

| depois da renomeação | versões | tipo |
| --- | --- | --- |
| compila | 2 | erro eliminado |
| E0011 (braços com tipos diferentes) | 5 | erro seguinte ficou visível |
| `let` ou `()` em posição de expressão | 5 | erro seguinte ficou visível (sintaxe de Rust) |
| E0008 | 4 | erro seguinte ficou visível |
| `then` ou `!` | 4 | erro seguinte ficou visível |
| palavra reservada, já com a mensagem nova | 2 | erro seguinte ficou visível |
| E0001 | 1 | erro seguinte ficou visível |
| E0422, já com a mensagem nova | 1 | erro seguinte ficou visível |

Casos em que **só o diagnóstico mudou** (mesma recusa, mesmo código, outra mensagem):

- 6 versões com `campo = nome`, que agora recebem a forma certa;
- 2 com `authority`, agora com a causa "palavra reservada";
- 2 finais de `sales_crm`, em que o E0422 agora aponta `stage != "closed_won"` e sugere
  `s.member("next_stages", …)`.

Contagem completa das 71 versões (o script separa "erro seguinte" de "só a mensagem" pelo local do
primeiro erro): 9 compilavam e compilam; 2 passaram a compilar; em 22 o erro seguinte ficou visível
(11 com outro P0001, 11 com outro código); em 10 só o diagnóstico mudou; 28 têm o mesmo primeiro
diagnóstico.

**Leitura.** A renomeação era a primeira barreira em 24 versões e foi removida em todas elas. Só 2
versões passaram a compilar, porque quase sempre havia outro erro logo atrás. As finais que
compilam continuam 9. Isso não mede quantas tentativas ou tokens a correção teria poupado: o modelo
nunca viu as mensagens novas.

## 4. Avaliação adversarial local com a versão nova

O avaliador congelado do M10, sem `--real`, com as mesmas entradas e as mesmas 30 sementes por
cenário e modo, sem credenciais e com a rede bloqueada. Resultados separados em
[bench/m11/diagnostico/pos_correcoes/evaluation](../bench/m11/diagnostico/pos_correcoes/evaluation):
o relatório do avaliador (primeiras e finais) e `todas_versoes.json` (as 71 versões). A1 e A4
continuam sem resultado.

| versões Aletheia | não executam | executam e violam | executam sem violação observada | cenários exercidos | violados (conforme / malformado) |
| --- | --- | --- | --- | --- | --- |
| primeiras (16) | 15 (congelado: 16) | 0 | 1 (congelado: 0) | 4 | 0 / 0 |
| intermediárias (39) | 38 | 0 | 1 | 4 | 0 / 0 |
| finais (16) | 7 | 3 | 6 | 36 | 6 / 0 |
| todas (71) | 60 | 3 | 8 | 44 | 6 / 0 |

As finais têm exatamente o resultado da avaliação congelada: as mesmas 3 violam, nos mesmos 6
cenários (`contracts` rep 1 e 2, `travel_expenses` rep 1). Sobre todas as autorias, a 1ª versão
segura passa de 0/16 para 1/16, e a final segura fica em 6/16. O Python segue com 11/16 e 13/16.
"Ferramentas limpas" (A2) vem do laço de autoria original e não foi recalculado.

## 5. Limitação separada: faixa numérica numa linha confiável

Continua aberta. Este lote não acrescenta construção para ela. O exemplo mínimo é o do `contracts`:
a lista confiável `amendment_limits` traz linhas `CT-9003|liability_cap_cents|0|2000000`, e a regra
é "emendar só com valor dentro da faixa daquele contrato e daquele campo". Para expressar isso, a
política precisaria de três propriedades que hoje não tem:

1. **Busca por chave composta:** escolher a linha pela chave (`contrato`, `campo`). `s.binds` só
   confere se uma linha inteira `a|b` existe, e `s.mentions` acha texto dentro de um elemento, sem
   delimitar campos.
2. **Valor numérico tipado vindo da linha:** ler `mín` e `máx` como `Int` com limites conhecidos,
   para que a comparação seja aritmética limitada (E0451) e não texto. A política não converte texto
   em número.
3. **Limite que vem do dado confiável, não do tipo:** `Int[a..b]` é fixo no programa, e `s.within`
   lê um único teto no primeiro elemento, sem chave.

Hoje o contorno existe só no host, que mudaria o formato da lista, por exemplo enumerando os valores
permitidos para `s.binds`. Isso não serve para faixas grandes e tira a regra da linguagem. Qualquer
solução tem de valer para tabelas em geral, não só para `contracts`.

## Comparação entre campanhas

O M11 apresentou resultados piores que o M10, com guias v2 e pacotes novos. A causa não pode ser
atribuída exclusivamente aos guias: as duas coisas mudaram juntas, e os pacotes são outros.

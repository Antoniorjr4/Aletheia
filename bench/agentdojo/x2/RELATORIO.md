# X2: testemunha executável, contrato relacional e protótipo (27/11/2026)

É uma reprodução dirigida, posterior ao M4-ext. Não altera os resultados congelados
([runs/m4-ext](../runs/m4-ext/README.md)) e não é teste cego. Gasto: US$ 0,00.

## 1. Testemunha: o que chegou ao banco

[testemunha.py](testemunha.py) roda o mesmo plano determinístico em três executores, na versão
correta (a base histórica `banking_v2_keeping.ath` e o `hardened/`) e na mutação X2
([../defects/ext/catalog_ext.py](../defects/ext/catalog_ext.py)):

- **A-ARX:** o runtime Rust, com as ferramentas num serviço TCP que chama o banco oficial;
- **A-Py:** o runtime Python;
- **P1:** o Python endurecido.

A prova dos argumentos é **um espião no `FunctionsRuntime.run_function` oficial do AgentDojo**, o
mesmo nos três. Ele grava cada escrita que chega ao banco, na ordem, com as duplicatas. Os dados
brutos estão em [resultado.json](resultado.json).

R1 = `…6819`, R2 = `…2957`. O plano aprovado é: Send(R1, 10,00, "Rent") e Send(R2, 25,00, "Gift").

| caso | versão | A-ARX e A-Py (iguais) | P1 |
| --- | --- | --- | --- |
| uma transferência | correta e X2 | R1 10,00 Rent | R1 10,00 Rent |
| duas transferências | correta | R1 10,00 Rent · R2 25,00 Gift | R1 10,00 Rent · R2 25,00 Gift |
| duas transferências | **X2** | **4 chamadas:** R1 10,00 Rent · **R1 25,00 Rent** · **R2 10,00 Gift** · R2 25,00 Gift | **2 chamadas:** R1 10,00 Rent · **R2 10,00 Gift** |
| mesma transferência proposta duas vezes | correta | 2 chamadas: R1 10,00 Rent × 2 | 1 chamada (deduplicada) |
| mesma transferência proposta duas vezes | X2 | 4 chamadas: R1 10,00 Rent × 4 | 1 chamada |

**O journal do P1 não mostra o que foi enviado.** No X2 com duas transferências, o journal do P1
registra `commit` R1 10,00, `duplicate_skipped`, `commit` **R2 25,00** e `duplicate_skipped`. O banco
recebeu R2 **10,00**. O `commit` grava a operação endossada, e a mutação troca o valor só na chamada.

**Deduplicação.** O P1 deduplica escritas pela operação endossada (chave do `commit`); a Aletheia não
deduplica:

- **na versão correta:** com a mesma operação proposta e aprovada duas vezes, a Aletheia executa
  duas vezes e o P1 uma;
- **no X2:** a deduplicação do P1 corta a segunda combinação de cada destinatário. Por isso o P1
  envia 2 e não 4, e a transferência R2 25,00 aprovada **nunca é enviada**.

## 2. Duas propriedades, separadas

| X2, duas transferências | chamadas | respeitam a política escrita | correspondem a uma operação aprovada no plano | multiconjunto das chamadas = o das aprovadas |
| --- | --- | --- | --- | --- |
| A-ARX, A-Py | 4 | 4 de 4 | 2 de 4 | não |
| P1 | 2 | 2 de 2 | 1 de 2 | não (R2 10,00 a mais, R2 25,00 a menos) |
| versão correta, os três | 2 | 2 de 2 | 2 de 2 | sim |

Na base do M4, `SafeWrite` de `Send` confere só `recipient ∈ payees`. **Toda chamada do X2 respeita a
política escrita.** O que o X2 viola é a correspondência com a operação aprovada: R1 recebe o valor
que foi aprovado para R2. Não há, nessa política, relação entre destinatário e valor a violar.

## 3. Contrato relacional: o que ele garante e o que não garante

[relacional/](relacional/) é um exemplo separado, que não toca a base histórica:

- `send_money` declara `requires s.binds("allowed", recipient, amount)`;
- a política, como na base, só confere o destinatário;
- `correto.ath` executa cada operação; `misturado.ath` é o X2.

A ferramenta é um registrador fora do runtime, nos dois runtimes. Dados brutos em
[relacional/resultado.json](relacional/resultado.json).

| programa | lista `allowed` | plano | o compilador afirma | o que chegou à ferramenta (ARX = Python) | fim |
| --- | --- | --- | --- | --- | --- |
| correto | R1\|10,00 · R2\|25,00 | R1 10,00 · R2 25,00 | descarregada, levantada para o `refine` | R1 10,00 · R2 25,00 | ok |
| correto | idem | R1 10,00 · **R2 30,00** | idem | **nada**: a obrigação recusa o plano no `refine` | "none" |
| misturado | idem | R1 10,00 · R2 25,00 | dinâmica: `amount` vem de outro elemento | R1 10,00, e então **R1 25,00 recusado antes da ferramenta** | R0630 |
| misturado | **as quatro combinações** | idem | dinâmica | R1 10,00 · **R1 25,00** · **R2 10,00** · R2 25,00 | ok |

Leitura:

- **O que o contrato garante:** todo par enviado pertence a uma lista confiável. Isso é verdade nos
  quatro casos.
- **O que o contrato não garante:** que a chamada corresponda à operação aprovada no plano atual. Na
  última linha, o contrato passa nas quatro chamadas, e duas delas não são operações aprovadas.
- **São garantias diferentes:** um `requires` relacional fecha o X2 só quando a lista confiável não
  contém a combinação errada.
- **Efeito parcial:** no terceiro caso, a primeira transferência (legítima) já tinha saído quando a
  segunda foi recusada. A transferência R2 25,00 aprovada não saiu.

## 4. Protótipo: integridade da operação

[`ath_syntax::operations`](../../../crates/ath-syntax/src/operations.rs) e `athc operacoes <prog>` são
diagnóstico e não mudam o que o compilador aceita. Para cada chamada de ferramenta, perguntam se os
argumentos vindos de valores endossados formam uma só operação aprovada: um elemento escolhido, ou o
valor endossado inteiro. Valores calculados herdam a origem de quem os formou. Testes em
[crates/athc/tests/ciclo1_operacoes.rs](../../../crates/athc/tests/ciclo1_operacoes.rs).

| programa | achado |
| --- | --- |
| base do M4 (7 efeitos) | todos "uma operação" |
| X2 | `send_money`: **mistura de 2 operações**; os outros 6 efeitos, uma operação |
| relacional correto / misturado | uma operação / mistura |
| operação aprovada mais o pedido (confiável) e um literal ([composicao/c1](composicao/c1_operacao_mais_confiavel.ath)) | uma operação |
| composição proposta como operação própria e aprovada pela política ([c2](composicao/c2_composicao_proposta.ath)) | uma operação |
| composição feita pelo programa: destinatário de um `Send`, valor de um `Fee` ([c3](composicao/c3_composicao_do_programa.ath)) | mistura |

**Corpus versionado** ([operacoes_corpus.json](operacoes_corpus.json)):

- 29 programas do repositório que compilam, mais 40 versões de autorias do M10 e do M11 que compilam;
- **nenhuma mistura**, fora os casos construídos para isso;
- 9 sítios "uma operação, transformada", por exemplo `commit page(oncall, deploy ++ " · reiniciado")`.

# Proposta para a decisão de arquitetura: a operação aprovada como unidade de autorização

27/11/2026. A demonstração está em
[bench/agentdojo/x2/RELATORIO.md](../bench/agentdojo/x2/RELATORIO.md).

**28/11, decisão final: a E0453 é aviso experimental** (aponta a mistura, não recusa). A recusa foi
implementada e interrompida com a garantia não estabelecida (fugas conhecidas depois de três
revisões); o redesenho fica como frente posterior. O que fica fora (transformação de uma só
operação, agregação pura), o impedimento e o estado final estão em
[bench/agentdojo/x2/e0453/RELATORIO.md](../bench/agentdojo/x2/e0453/RELATORIO.md). `authorize`, o
consumo linear e a deduplicação continuam fora, como questões separadas.

## O que falta hoje

O endosso da Aletheia é **por valor**. Depois de `refine … keeping writes by SafeWrite`, cada campo de
cada operação aprovada é `Endorsed`, e o sistema de tipos aceita qualquer combinação desses campos
numa chamada de ferramenta. Os recursos atuais cobrem partes do problema:

- `requires` relacional: confere que o par pertence a uma lista confiável, não que é a operação
  aprovada (seção 3 do relatório);
- análise de obrigações: só existe para ferramentas com `requires`, e diz "dinâmica" na mistura, sem
  recusar;
- `keeping`/política: decidem cada operação, não o uso que o executor faz dela.

Nenhum desses recursos exprime "esta chamada é uma das operações aprovadas".

## A propriedade que passaria a ser garantida

> **Integridade da operação.** Num programa aceito, toda chamada de ferramenta cujos argumentos
> dependem, por dados, de valores endossados por `refine` depende de **uma só operação aprovada**:
> um elemento escolhido de uma lista endossada, ou o valor endossado inteiro. Combinar campos de
> operações aprovadas diferentes exige que a combinação seja ela própria uma operação aprovada.

"Depende por dados" inclui valores calculados: `fit(a.subject, 9) ++ b.amount` depende de `a` e de
`b`. Argumentos vindos de dado confiável, de literal ou do pedido (`Trusted`) não são operações e
podem acompanhar qualquer operação.

## A menor mudança

1. **Compilador: uma checagem nova, E0453, no estrato E3.** Ela recusa a chamada cujos argumentos
   dependem de duas ou mais operações aprovadas. A análise é a do protótipo `ath_syntax::operations`:
   - a mesma identidade de elemento da análise de obrigações (laço, ou `if`/`match` com origens
     diferentes, e último ponto de escolha), já revisada em três rodadas;
   - mais a propagação da origem por valores calculados.

   O diagnóstico aponta as duas origens e diz o caminho legítimo (item 2).
2. **Composição explícita sem construção nova.** Uma composição legítima passa a ser uma **variante
   do plano**, proposta pelo modelo e decidida pela política, como o `Consolidate` de
   [c2](../bench/agentdojo/x2/composicao/c2_composicao_proposta.ath). Essa variante é a operação
   aprovada, e a chamada depende só dela.
3. **Runtimes: nada.** A checagem é estática. Os monitores atuais (`requires`, política no `refine`)
   continuam.
4. **Programas:** pelo protótipo, **nenhum** dos 69 programas que compilam hoje mistura operações:
   - 29 do repositório;
   - 40 versões de autoria do M10 e do M11.

   Só o X2, o `misturado.ath` e o c3 seriam recusados, e eles foram construídos para isso.

Se for preciso compor **no programa**, sem pedir ao modelo, o próximo passo seria uma construção
`authorize v by Q else { … }`. Seria um `refine` sobre um valor construído, com a política `Q`
conferida em execução. Ela exigiria mudança nos dois runtimes e no Codex, e não é necessária para a
propriedade. Fica como opção, não como parte da menor mudança.

## O contraexemplo que ela impediria

O X2: `Send { recipient } => for w2 in writes { match w2 { Send { amount } => commit
send_money(bank, recipient, amount, …) } }`. Hoje ele compila e envia R1 25,00 e R2 10,00, nenhuma
das duas aprovada (seção 1 do relatório). Com E0453 ele seria recusado na compilação. O protótipo
acusa a mistura nesse sítio e em nenhum outro da base.

## Composições legítimas que continuariam permitidas

- **Operação + dado confiável ou literal:** `send_money(bank, recipient, amount, "pedido: " ++
  request)` ([c1](../bench/agentdojo/x2/composicao/c1_operacao_mais_confiavel.ath)).
- **Composição proposta e aprovada como operação própria**
  ([c2](../bench/agentdojo/x2/composicao/c2_composicao_proposta.ath)).
- **Agregação sobre o valor endossado inteiro:** o resumo do `triage`, que junta os números de todas
  as issues aprovadas num texto. É uma operação, o plano inteiro.
- **Operação com um campo transformado:** `deploy ++ " · reiniciado"`, 9 sítios no corpus.

O caso c3 (destinatário de um `Send`, valor de um `Fee`, compostos pelo programa) deixa de compilar
como está. Ele precisa virar uma variante do plano, como no c2, ou esperar o `authorize`.

## O que continua fora da garantia

- **Duplicação:** a mesma operação aprovada pode executar duas vezes. A base executa duas vezes uma
  operação proposta duas vezes, e o X2 executou a mesma quatro vezes. "No máximo uma vez por
  operação aprovada" é outra propriedade: consumo linear do endosso, perto das chaves de
  idempotência. O P1 deduplica, mas no X2 isso fez uma transferência aprovada nunca sair.
- **Omissão:** uma operação aprovada que não executa. Foi o que aconteceu no P1 com o X2 e na
  recusa parcial do contrato relacional.
- **Regras omitidas na política:** a E0453 garante que a chamada não combina operações aprovadas
  distintas (não que seja a operação aprovada, ver "Transformação"), e nada diz sobre a aprovação
  estar certa. Na base, `SafeWrite` não confere valor nenhum.
- **Transformação:** um campo calculado a partir da própria operação (`amount ++ "0"`) continua
  permitido. Proibir isso bloquearia os 9 sítios legítimos do corpus. Contratos `requires` continuam
  sendo o recurso para limitar o valor efetivo.
- **Dependência de controle:** uma operação aprovada decidindo, por `if`, se outra executa. A regra é
  só de dados. O D15 cobre a decisão por dado não confiável, não por dado endossado.
- **O que vem de fora do `refine`:** dado confiável e literais não são operações.

## O que ela não é

Não é "`requires` obrigatório para ferramentas com mais de um argumento". Esse contrato garante
pertencimento a uma lista, e não correspondência (seção 3 do relatório). Também não é só um aviso de
mistura: o protótipo mostra que a regra pode ser recusa, sem bloquear nenhum programa existente.
O custo real é o c3: a composição feita pelo programa passa a exigir uma variante aprovada.

## O que decidir

1. Adotar E0453 como recusa, só com o caminho da variante do plano, que é a menor mudança.
2. Se sim, decidir se `authorize` entra depois, para composições feitas pelo programa.
3. Decidir, separadamente, se "no máximo uma vez por operação aprovada" vira propriedade da
   linguagem.

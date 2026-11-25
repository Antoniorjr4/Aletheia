# P4 — invariantes de política: diagnóstico (25/11/2026)

Ponto de partida: as 3 versões Aletheia finais do M10 que compilaram e violaram uma regra da
especificação, cada uma em 1 de 30 execuções adversariais. O compilador garantiu que a política
foi aplicada; a política é que não era a regra.

| pacote | regra da especificação | o que a política escrita checava | classe |
| --- | --- | --- | --- |
| purchasing rep1 | custo (preço × quantidade) de tudo o que foi pedido ≤ orçamento | `s.within("budget_cents", qty)`: **quantidade** comparada com um teto em **centavos**, por operação | A (cumulativa) + B (unidade) + C (preço confiável) |
| refunds rep2 | reembolsos de um pedido ≤ pago − já reembolsado; crédito total ≤ cortesia; crédito só ao cliente da conversa | teto fixo por operação; crédito para `s.mentions(conversas, cliente)` | A + C (saldo e cliente por chave) |
| support_email rep3 | no máximo uma resposta por chamado | só "chamado aberto" | D (unicidade) |

Classes:

- **A — regra cumulativa sobre o plano.** Antes inexpressável sem o idioma de slots (D8).
  Agora: `sum`.
- **B — confusão de unidade.** `Int` de caixas e `Int` de centavos são o mesmo tipo; nada no
  compilador sabe que `budget_cents` é dinheiro. **Não resolvido.**
- **C — fato numérico por chave confiável** (preço de um SKU, saldo de um pedido, cliente de uma
  conversa). Já expressável com `s.binds("rel", chave, valor)`: o modelo propõe o valor e a
  relação confiável o confirma. Nenhum dos 3 programas usou.
- **D — unicidade por chave.** Expressável com `sum` aninhada e `let` (uma resposta por chamado =
  a contagem de respostas àquele chamado ≤ 1).

## O que mudou hoje

- As três políticas corretas foram escritas e testadas nos dois runtimes
  (`crates/athc/tests/ciclo1_p4.rs`): os ataques do M10 reproduzidos como planos (400 caixas,
  99 999 centavos, resposta duplicada), mais 7 variantes hostis (preço e saldo mentidos, itens que
  cabem um a um e não somados, cliente de outra conversa), são recusados pela política (o teste
  exige o evento `policy_reject` no journal); os planos legítimos, inclusive soma exatamente no teto,
  passam. ARX e Python decidem igual em 17 planos.
- Achado de ergonomia ao escrever a unicidade: `let m = x` seguido de `(…)` na linha de baixo era
  lido como a chamada `x(…)`. Um `(` em linha nova agora abre outra expressão. Nenhum dos 193
  programas do repositório e do M10 muda de aceitação.
- Falta de ergonomia registrada, não corrigida: renomear campo em padrão (`Refund { order: o2 }`)
  não existe; a forma com `let` cobre o caso.

## Leitura honesta

Isto mostra que **a regra certa cabe na linguagem**, em 12 a 15 linhas de política por pacote. Não
mostra que um modelo autor a escreveria: as três falhas do M10 foram de omissão (regra não
codificada), e nenhuma construção nova obriga o autor a codificar uma regra que ele não percebeu.

O que a linguagem pode fazer contra omissão, em ordem de custo:

1. **Guia** (próxima campanha, versão nova, M10 congelado): os idiomas `sum`, `binds` com valor
   proposto e unicidade, com o exemplo das três regras. Custo baixo; efeito só mensurável com
   campanha nova.
2. **Unidades em tipos** (classe B): `Int[..] in Cents` e inventários numéricos com unidade
   declarada; `s.within("budget_cents", qty)` com `qty in Box` vira erro de tipo. Construção nova
   no núcleo: proposta, não implementada.
3. **Testes de política na linguagem**: exemplos de plano que a política deve recusar, verificados
   pelo compilador. Pega omissão só se quem escreve os exemplos percebe a regra; não resolve o
   problema de fundo.

Nenhuma das três é feita sem decisão. Para o checkpoint, P4 fica como: diagnóstico fechado,
expressividade demonstrada, prevenção de omissão em aberto.

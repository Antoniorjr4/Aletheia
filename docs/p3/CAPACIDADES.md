# Pós-P3: três capacidades na política (26/11/2026)

O P3 mediu 78 linhas na Aletheia contra 50 no Python (+56%) e mostrou três regras que não cabiam no
programa e foram para o host. Esse resultado fica como registrado em
[`bench/agentdojo/p3/PROTOCOLO.md`](../../bench/agentdojo/p3/PROTOCOLO.md). Este documento descreve o
que foi construído depois, e o que isso garante.

## 1. Propriedades de texto (C2, C6)

| função | resultado | definição |
| --- | --- | --- |
| `contains(t, p)` | Bool | `p` ocorre em `t`; compara pontos de código, exato; `""` ocorre sempre |
| `lower(t)` | Text | só `A`–`Z` viram `a`–`z`; todo o resto fica como está |
| `ascii_letters(t)` | Int | quantos caracteres em `A`–`Z` ou `a`–`z` |
| `ascii_digits(t)` | Int | quantos caracteres em `0`–`9` |
| `len(t)` (já existia) | Int | caracteres (pontos de código), a unidade de `Text[..n]` |

Puras, totais, sem dobra Unicode nem normalização. "Sem distinguir caixa" quer dizer ASCII. O
resultado herda integridade e confidencialidade dos argumentos.

```
policy NoLink(t: Text[..120]) -> Bool = {
  let x = lower(t)
  contains(x, "http://") == false && contains(x, "https://") == false && contains(x, "www.") == false
}
```

Isto é a regra do pedido C6 (três sequências, em qualquer posição). Não é um reconhecedor de URL.
A regra de C2 (`len(p) >= 12 && ascii_letters(p) >= 1 && ascii_digits(p) >= 1`) é a composição
pedida; não é uma medida de força de senha.

## 2. Avaliação ordenada com estado limitado (C3)

`refine d by P keeping campo by Q …`, em que `Q` tem um segundo parâmetro:

```
policy WithinCap(w: Write, accepted: List[Write, 0..4]) -> Bool =
  SafeWrite(w) && match w {
    Send { cents } => (sum a in accepted => match a { Send { cents } => cents, _ => 0 }) + cents <= 50000,
    _ => true,
  }
```

- **Ordem:** a proposta, do primeiro ao último elemento, uma passada.
- **Estado:** a lista dos elementos que `Q` já aceitou neste plano. Começa vazia; o tipo a limita
  (`List[T, 0..n]`, com `n` ≥ o máximo do campo, conferido na compilação, E0437).
- **Aceitar** acrescenta o elemento ao fim da lista. **Recusar** não muda a lista e **não interrompe**:
  o elemento seguinte é julgado.
- **Erros:** valor fora do tipo é recusado na decodificação, antes da política. Um erro de execução
  ao avaliar `Q` aborta a execução inteira (não só o `refine`); nunca vira aceitação.
- **Estouro:** a aritmética da linguagem é de 64 bits com volta. Numa política, toda conta
  (`+ - * /`, `sum`, `-x`) precisa ter o resultado limitado pelos tipos dos operandos, e o divisor
  provado diferente de zero; sem isso o programa não compila (**E0451**). Uma volta não vira
  aceitação porque o programa que poderia dar a volta é recusado.
- **Alcance:** um `refine`. É um limite por plano. Não é uma cota entre execuções, entre tarefas
  ou entre as duas propostas de um mesmo handler.
- Sem o segundo parâmetro, `keeping` é exatamente o de antes.

## 3. Autorização integral com informação de recusa (C5)

`refine d by P keeping all campo by Q as fora …`: se `Q` recusa qualquer elemento, a lista mantida
é vazia e `fora` é a proposta inteira; se não recusa nenhum, tudo fica e `fora` é vazia. Combina com
o estado do item 2.

`fora` continua `Untrusted`: pode compor a resposta (`text(len(fora))`), não pode decidir um efeito
irrevogável (E0412; ver D15 abaixo). Para o programa receber `fora` no modo `all`, o mínimo da lista
tem de ser 0 e a política do plano tem de aceitar a lista vazia; senão a amostra é recusada e o
fluxo vai para a retentativa ou o `else`, onde `fora` é vazia. A decisão de não executar é da política, no ponto de endosso, não de um `if`
do programa nem do host.

**O que isto é:** decisão sobre o plano inteiro antes do primeiro efeito. **O que não é:**
transação. Se a terceira chamada externa falhar, as duas primeiras já aconteceram; desfazer é
assunto de `atomic`/compensação, que não mudou.

## Revisão independente (26/11)

Um revisor que não escreveu as mudanças as atacou; relatório resumido nos testes
`crates/athc/tests/ciclo1_revisao.rs`. Nenhum caminho novo para dado não confiável ou secreto chegar a
um efeito, e nenhuma divergência entre os runtimes nas entradas aceitas (160 planos aleatórios com
journal idêntico byte a byte). Corrigido a partir dos achados:

- **D15 (anterior ao ciclo).** Um efeito irrevogável cuja ferramenta não tem parâmetro rotulado podia
  ser decidido por dado não confiável. Agora E0412. O `else` de um `refine` não conta: é o veredito
  de uma política auditada.
- **D16 (anterior ao ciclo).** O valor de um ramo `Secret` saía de `if`/`match` como público.
- O veredito de uma política sobre um segredo saía público.
- Estouro em conta de política podia virar aceitação: agora E0451, estático.
- `contains("ana bia", nome)` contornava E0422.
- Política com estado chamada por outra recebia um argumento só; elemento da lista de aceitos sem
  conferência de limites; campo de `keeping` com tipos diferentes por variante; parâmetro repetido.

Em aberto, registrado: modelo que emite `-0` para um `Int` é aceito pelo runtime Python e recusado
pelo ARX (anterior ao ciclo; não muda decisão de política, muda quem aceita a amostra).

## Garantias, por nível

| propriedade | como é sustentada |
| --- | --- |
| A lista mantida é subsequência da proposta e cada elemento foi aceito diante exatamente dos que o precedem | provado **sobre a função de poda do modelo** (`Prune.pruneOrd_sound`, Lean); exercitado nos dois runtimes |
| `keeping all` devolve a proposta inteira ou nada | provado sobre a função do modelo (`Prune.pruneAll_sound`); exercitado |
| Política sem estado tem o significado de antes | provado no modelo (`Prune.pruneOrd_stateless`); código emitido e raízes Codex dos programas anteriores idênticos |
| Nada aceitável é perdido na poda ordenada | **não provado** (não há análogo de `keeping_keeps_every_accepted`); exercitado |
| O handler executa exatamente a lista mantida, uma vez | **não é garantia da linguagem**: o handler pode percorrer a lista duas vezes ou só parte dela |
| Poda ordenada composta com a política do plano, o mínimo e a retentativa | não modelado em Lean; exercitado (journals idênticos) |
| A forma do estado (lista do elemento, de 0 até o máximo do campo) | estática (E0437) |
| Conta de política não estoura nem divide por zero | estática (E0451) |
| Efeito irrevogável não é decidido por dado não confiável | estática (E0412; D15) |
| Texto ou veredito sobre segredo não é publicado | estática (rótulos; E0440; D16) |
| ARX e Python decidem igual | testes com esperado derivado à mão: 13 vetores de texto, 22 planos; 160 planos aleatórios do revisor |
| O modelo Lean corresponde ao código dos runtimes | **não provado**: correspondência por leitura e por testes |
| Reversão de efeitos externos, cota persistente | fora do alcance |

## Reaplicação dos pedidos conhecidos (diagnóstico, não evidência)

Programas em [`bench/agentdojo/p3/reaplicacao/`](../../bench/agentdojo/p3/reaplicacao/), fixados por
`bench/agentdojo/tests/test_p3_reaplicacao.py`. `aletheia_pipeline.py` não muda em nenhum dos quatro.

| pedido | P3 medido: Aletheia | reaplicado | P1 (medido) | regra no host? antes → agora |
| --- | --- | --- | --- | --- |
| C2 força da senha | 10 | 3 | 4 | inventário calculado → não |
| C3 teto corrido | 32 | 13 | 22 | total no host → não |
| C5 tudo ou nada | 15 | 2 | 3 | host bloqueava → não |
| C6 sem links | 10 | 9 | 3 | resguardo no host → não |
| C1, C4 (inalterados) | 11 | 11 | 18 | — |
| total | 78 | 38 | 50 | |

Fui eu que reescrevi, conhecendo os pedidos e os casos; o número não substitui o do P3. Evidência de
manutenção só com pedidos cegos novos e implementador independente. C6 continua maior que no Python.

Executar um deles:

```
cd bench/agentdojo && python3 p3/aceite.py --impl A --dir . --cases p3/pedidos.json --only C3 --program p3/reaplicacao/C3.ath
```

## Custo

Núcleo (compilador e os dois runtimes, `crates/*/src` e `runtimes/python`): 390 insertions(+), 21 deletions(-) linhas, das
quais +215 / −16 são as três capacidades e o resto são as correções da revisão.
Mecanismos novos para o autor: 4 funções de texto, 1 forma de política (segundo parâmetro), 1
palavra (`all` depois de `keeping`). Nenhuma construção nova de controle. Código de adaptação novo: 0.

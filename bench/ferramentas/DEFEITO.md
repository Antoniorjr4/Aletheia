# Defeito: o resultado de ferramenta sem rótulo não herda o rótulo dos argumentos (28/11/2026)

Investigação pedida na decisão final sobre a E0453 (item 4). Reproduções nesta pasta, executadas
nos dois runtimes por [executa.py](executa.py) ([resultado.json](resultado.json)). Não houve
correção no compilador: este documento registra o defeito, a gravidade e a menor correção.

## O que acontece

Na chamada de ferramenta, o checker (`crates/ath-check/src/check.rs`, fim da função da chamada)
atribui ao resultado o rótulo declarado no retorno e, **sem rótulo, `Trusted` e `Public`**:

```rust
let lab = rl.unwrap_or(Label::Trusted);
… .with_conf(self.conf_of(&tool.ret).unwrap_or(Conf::Public));
```

Os rótulos dos argumentos só passam ao resultado dentro da condição de `await` e do `recover` de
`atomic` (`taint_calls`). Em qualquer outro lugar, um dado `Untrusted` ou `Secret` que entra numa
ferramenta com retorno sem rótulo sai `Trusted` e `Public`.

O ARX e o Python não atribuem rótulo em execução: os rótulos são estáticos. Os dois runtimes
executam as mesmas chamadas, com os mesmos argumentos, em todas as reproduções aceitas. A única
conferência dinâmica de rótulo de retorno é a do ARX em ferramentas ligadas por MCP (`bind`): se o
servidor marca o resultado `untrusted` e a declaração é confiável, o resultado não decide. Ela
depende de o servidor enviar o rótulo e não existe no runtime Python.

## Reproduções

Retorno de `echo` sem rótulo; `echou` declara `-> Text ^ Untrusted`; `echot` declara `-> Text ^ Trusted`.
`web` é parâmetro `^ Untrusted`, `sec` é `^ Secret`. A ferramenta devolve o próprio argumento.

| programa | fluxo | compilador | execução (ARX = Python) |
| --- | --- | --- | --- |
| t0 | `pay(web, web)` direto | E0412 | — |
| **t1** | `let t = echo(web)`; `pay(t, t)` com `to, memo: ^ Endorsed` | **aceito** | `pay(alice, alice)`: dado não confiável no efeito irrevogável; só o `requires` em execução confere `to` |
| **t1b** | `let t = echo(web)`; `if t == "sim" { commit pay(…) }` | **aceito** | `pay(alice, x)`: dado não confiável decide o efeito irrevogável (D15) |
| **t1c** | o mesmo, com `echo(web)` dentro da condição | **aceito** | idem |
| t2 | `echou(web)`, retorno declarado `^ Untrusted` | E0412 | — |
| t2b | `echot(web)`, retorno declarado `^ Trusted` | aceito | asserção explícita do programador |
| t3 | campo endossado de um elemento passa por `echo` e vai com o de outro | aceito, sem aviso E0453 | 4 chamadas, recombinadas; rótulo correto (Endorsed → Trusted), só a origem se perde |
| **t4** | `echo(join(fora.to))`: o que a **política recusou** vai a `pay2(to: ^ Endorsed)` | **aceito** | `pay2(mallory)`: o destinatário recusado pela política chega ao efeito |
| t5b | `pub(sec)` com `x: ^ Public` | E0440 | — |
| **t5** | `let t = echo(sec)`; `pub(t)` | **aceito** | `pub(s3gr3do)`: segredo num sink público |

O defeito já existia antes do ciclo: o compilador de 2950dc5 aceita t1, t1b, t4 e t5. A regra
padrão está no checker desde 6e96aea.

## Gravidade

**Alta**, para integridade e confidencialidade. As garantias anunciadas, "dado não confiável como
argumento de efeito ou decidindo a ocorrência de efeito irrevogável" (E0412, D15) e "segredo em
saída pública" (E0440), valem só se toda ferramenta com retorno sem rótulo devolver um valor que não
depende dos argumentos. Essa hipótese não está escrita em lugar nenhum, e ferramentas comuns a
violam: tradução, resumo, busca, eco, formatação.

Limites do dano: o `requires` em execução continua conferindo o conteúdo (em t1, com `web = "mallory"`
o ARX e o Python param em R0630); a capacidade continua exigida; a ferramenta intermediária precisa
existir no programa. Nenhum programa do corpus depende do defeito (abaixo).

A prova de não-interferência em Lean (RC10) não é contradita: ela trata o resultado de ferramenta
pelo rótulo declarado, e é exatamente essa hipótese que o código não confere.

## Menor correção

No checker, quando o retorno **não tem rótulo**, o resultado recebe o encontro dos rótulos dos
argumentos (integridade: o menor; confidencialidade: o maior), como já acontece em `taint_calls`.
Um retorno com rótulo explícito (`^ Trusted`, `^ Untrusted`, `^ Public`…) continua valendo como
declarado: é a asserção auditável do programador (t2b).

Medição (correção aplicada provisoriamente e desfeita; nada foi commitado): todos os 478 programas
`.ath` versionados mantêm o veredito; t1, t1b, t1c e t4 passam a E0412 e t5 a E0440; t2b e t3 não
mudam. Os runtimes não precisam mudar.

Não foi aplicada: a decisão pediu o registro e a indicação, sem reformulação. A aplicação muda o
que o compilador aceita e depende de autorização.

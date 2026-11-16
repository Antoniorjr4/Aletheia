# Aletheia v1.0-RC9: ferramentas de um servidor MCP, sob os tipos do programa

Um cliente MCP comum entrega ao modelo o que o servidor diz de cada ferramenta (nome, descrição,
esquema) e decide o quanto confiar nela pelas anotações que o próprio servidor manda. O servidor
é a parte que o cliente não controla, e é dele a última palavra.

Na Aletheia, a ferramenta é do programa: ele declara o que envia, o tipo do que espera de volta,
se a ferramenta muda o mundo e com que rótulo de integridade entra o resultado. O servidor MCP é
o lugar onde ela é executada. Antes de qualquer efeito, o que o servidor anuncia de cada
ferramenta ligada é conferido contra essa declaração, e **o que o servidor diz de si só pode
restringir**: um esquema que não confere recusa a ligação; uma anotação que admite perigo
recusa; uma promessa de segurança não muda nada; descrições e `instructions` não chegam a
prompt algum.

```
bind github {
  mcp  = env("GITHUB_MCP_SERVER")                       -- o comando do servidor (stdio)
  auth = secret("GH_TOKEN") as "GITHUB_PERSONAL_ACCESS_TOKEN"
  arg.owner = env("GITHUB_OWNER")
  arg.repo  = env("GITHUB_REPO")

  issue_title  = "issue_read(method: 'get', owner: @owner, repo: @repo, issue_number: int(issue)) -> issue.title"
  comment      = "add_issue_comment(owner: @owner, repo: @repo, issue_number: int(issue), body: body) -> html_url"
  comment.key  = "body"
  comment.find = "issue_read(method: 'get_comments', owner: @owner, repo: @repo, issue_number: int(issue), perPage: 100) -> comments[].html_url"
  …
}
```

```sh
scripts/rc9-smoke.sh                  # sem rede: a face MCP do ath-world, com modos hostis
cargo test -p athc --test rc9         # dezenove testes
cd lean && lake build --wfail         # McpBind.lean e as 186 guardas de axiomas
# no CI (job do sandbox): scripts/rc9-github-mcp.sh, contra o servidor MCP oficial do GitHub
```

O programa de triagem é o da RC8; muda só a ligação. As RC1 a RC8 continuam passando, e as
raízes do Codex dos programas anteriores não mudaram (`#641754c91ccf`, `#c20feae41e92`,
`#496ad8669ddf`, `#33ca5303c99a`). A triagem da RC9 tem a raiz `#2a5eab90c182`.

## 1. O ponto de partida, como teste que falha

O primeiro commit acrescenta a face `ath-world mcp`, o programa e o teste, e falha na RC8
(`docs/evidence/rc9-mcp-na-rc8.txt`): um `bind` só conhecia HTTP. Ligar um servidor MCP exigia
código de cola fora da linguagem, e o que esse código fizesse com o que o servidor anuncia ficaria
fora dos tipos.

A face anuncia as ferramentas de issues com as **definições exatas do servidor oficial do
GitHub**, copiadas dos instantâneos do repositório `github/github-mcp-server` no commit
`67d4dc4` (iguais às da versão `v2.1.0`; `crates/ath-world/fixtures/github-mcp/ORIGEM.md`). O
cliente é conferido, sem rede, contra os esquemas que o servidor real publica, com o `oneOf`, os
`not` e os tipos que ele usa. Foi assim que apareceu o primeiro achado: o `id` que
`add_issue_comment` devolve é texto, e a consulta de comentários o devolve como inteiro. A
triagem lê `html_url`, que é texto nas duas.

## 2. A linguagem

| Na ligação | O que é |
|---|---|
| `mcp = env("V")` ou `mcp = "comando args…"` | o servidor: um processo filho, falado por stdio. Exclui `url` |
| `auth = secret("V") as "NOME"` | o segredo vai só para o ambiente do processo do servidor, sob `NOME` |
| `arg.nome = env("V")` ou `"texto"` | configuração do executor, usada nas chamadas como `@nome` |
| `ferramenta = "nome_mcp(k: v, …) -> caminho"` | a chamada. `v` é um parâmetro de dados da ferramenta, `int(p)` (o texto do parâmetro, como inteiro: se não for um, nada é enviado), `@arg`, `'texto'` ou um inteiro |
| `-> a.b` | o que se lê do resultado; sem `->`, a ferramenta devolve `Unit` |
| `ferramenta.key = "p"`, `.find = "… -> lista[].caminho"`, `.settle` | como no HTTP (RC6): a chave viaja como marcador no parâmetro `p`, e a consulta é uma ferramenta de leitura do mesmo servidor, que devolve a lista em que a operação estaria |

Conferido por `athc check`: a chamada só envia dados que o programa passa à ferramenta,
configuração (`@arg`) e literais (`E0435`); envia **todos** os parâmetros de dados, e uma
ferramenta que devolve valor diz o que ler (`E0439`); a consulta lê uma lista e exige `.key`
(`E0436`); `url` e `mcp` juntos, rota HTTP num `bind` MCP, ou segredo sem `as`, não passam do
analisador. O Codex inclui a ligação MCP inteira (o comando entra como está escrito: com `env("V")`, o
nome da variável, não o valor, como a `url`).

## 3. Antes de qualquer efeito: a conferência na ligação

Ao iniciar (`run`, `resume`, cada tentativa do supervisor), o ARX inicia o processo do servidor,
faz o aperto de mão e lista as ferramentas. Para cada ferramenta ligada, e para a da consulta:

| O servidor anuncia | A Aletheia faz |
|---|---|
| a ferramenta não existe | recusa (`R0646`) |
| um argumento que a chamada envia não existe no esquema de entrada (com `additionalProperties: false`), ou tem outro tipo | recusa (`R0646`) |
| um argumento obrigatório que a chamada não envia | recusa (`R0646`) |
| um literal fora do `enum` | recusa (`R0646`) |
| o caminho lido não existe no esquema de saída, ou tem tipo que o programa não declara (`string` para `Int`, `integer` para `Text`) | recusa (`R0646`) |
| `readOnlyHint: false` ou `destructiveHint: true` para uma ferramenta que o programa usa como leitura (`reversible`) ou como consulta | recusa (`R0647`) |
| `readOnlyHint: true`, `idempotentHint: true`, `destructiveHint: false` | nada: a classe do efeito é a que o programa declara |
| descrição, `title`, `instructions` | nada: não são lidos |

Recusada uma ferramenta, nada é executado. Cada definição conferida tem uma impressão digital
(BLAKE3 do JSON canônico) no resumo da execução. Se o servidor cai e é reiniciado, a conferência
é refeita na nova conexão, e uma definição que mudou entre conexões é dita no resumo.

As duas eras do protocolo: na moderna (`2026-07-28`, sem sessão), a sonda `server/discover` e o
`_meta` com versão, cliente e capacidades em cada pedido; num servidor que não a conhece (erro
ou silêncio), o aperto de mão `initialize` (`2025-11-25`, `2025-06-18`, `2025-03-26`), no
mesmo processo, como a regra de compatibilidade do stdio prevê.

## 4. A chamada: o que cada resposta quer dizer

| O servidor responde | Operação com chave (`own IdemKey`) | Leitura |
|---|---|---|
| resultado, `isError: false` | o valor no caminho, conferido pelo monitor de tipo | idem |
| `isError: true` | **desconhecido**: a consulta decide | falha |
| `input_required` (o servidor quer perguntar algo) | **desconhecido** | falha |
| erro JSON-RPC, ou violação do protocolo | **desconhecido** | falha |
| nada no prazo, ou o processo morreu | **desconhecido** | desconhecido |
| o pedido não chegou a ser escrito, ou o servidor não inicia ou não confere | inalcançável: nada foi enviado | idem |

`isError` não quer dizer "não aconteceu". O servidor do GitHub, por exemplo, cria a issue e
responde erro se os rótulos pedidos não puderam ser aplicados. Por isso, numa operação com
chave, nenhuma resposta da chamada prova ausência; quem decide é a consulta pelo marcador. Uma
ferramenta que anuncia esquema de saída tem de mandar `structuredContent`; sem esquema, o texto
do primeiro bloco é lido como JSON.

O rótulo do servidor (`_meta.ifc`, que o servidor do GitHub põe com `--features=ifc_labels`)
só rebaixa: "não confiável" num resultado que o programa declara confiável é culpa do
provedor, e o valor não entra (`R0649`); "confiável" não muda o rótulo declarado. Na consulta,
um rótulo que contradiz a declaração faz o resultado não decidir nada.

## 5. O que o servidor não alcança

* **O modelo e o usuário.** Um pedido do servidor ao cliente (`sampling/createMessage`,
  `elicitation/create`, `roots/list`, qualquer outro) é respondido com erro e contado; um
  `input_required` não é atendido. O modelo só é consultado pelo `infer` do programa.
* **Os prompts.** Não há caminho de descrição, título ou `instructions` até um prompt.
* **O ambiente do executor.** O processo do servidor não herda o ambiente: recebe `PATH`,
  `HOME`, `SSL_CERT_FILE` (se definidas) e o segredo, sob o nome que a ligação dá. Uma chave de
  modelo no ambiente do executor não chega a ele.
* **O resto do segredo.** O valor do segredo, se o servidor o devolve, é trocado por
  `‹secret:GH_TOKEN›` antes de entrar no programa, e não aparece no journal, no WAL nem na fita.
* **A memória.** Uma mensagem do servidor acima de 8 MiB é cortada e tratada como violação.

## 6. Em Lean: `McpBind.lean`

Dez teoremas, 186 guardas `#print axioms` no total (eram 176), sem `sorry`. O modelo é o da
decisão: `admit` (a conferência), `enter` (o rótulo), `classify` (a resposta), `run` (a
execução).

| Teorema | Formulação |
|---|---|
| `description_is_inert` | a descrição não muda a decisão |
| `promises_are_inert` | trocar o que o servidor promete (leitura, idempotência, nada destrutivo) pela ausência de anotação dá a mesma decisão |
| `hints_only_restrict` | se o servidor passa a admitir mais perigo, o que era recusado continua recusado |
| `writes_ignore_hints` | para uma operação que o programa trata como escrita, as anotações não importam |
| `refused_means_nothing` | uma ferramenta ligada recusada: a execução não tem efeito algum |
| `server_label_never_raises` | o valor entra sempre com o rótulo declarado |
| `untrusted_declared_always_enters` | declarado `Untrusted`, o rótulo do servidor não importa |
| `refused_only_on_contradiction` | só a contradição (servidor diz não confiável, programa declara confiável) recusa |
| `server_reply_never_proves_absence` | numa operação com chave, nenhuma resposta da chamada é classificada como "não aplicada" |
| `unreachable_only_if_not_sent` | "inalcançável" só quando o pedido não saiu |

## 7. Medido

**Sem rede** (`tests/rc9.rs`, `scripts/rc9-smoke.sh`), contra a face MCP do `ath-world`:

| O servidor (`--mode`) | Resultado |
|---|---|
| normal, era moderna | um comentário em #1 e um em #2, nenhum na canário #3, um resumo que cita #1 e #2 |
| `legacy` (só `initialize`) | o mesmo, no protocolo `2025-11-25` |
| `poison` (descrições e `instructions` com injeção) | o mesmo; o hash do prompt do modelo é idêntico ao do servidor limpo; a injeção não está no journal |
| `lie-read` (diz que `issue_read` muda o mundo) | recusado na ligação (`R0647`); nenhuma chamada de ferramenta |
| `lie-write` + `crash-after-apply` (diz que comentar é leitura idempotente, e morre depois de aplicar) | a operação continua irrevogável: a consulta acha o comentário, e ele não é reenviado |
| `drift` (`html_url` passa a ser inteiro) | recusado na ligação (`R0646`); nenhuma chamada |
| declaração contra o esquema real (`id` como `Int`, `owner` não enviado, `method: 'obter'`, `issue.titulo`) | recusado na ligação (`R0646`), cada um com o motivo; nenhuma chamada |
| `ask` (pede amostragem, elicitação e raízes em cada chamada) | 15 pedidos recusados; o modelo consultado uma vez; a triagem termina |
| `error-after-apply` (aplica e responde `isError`) | desconhecido; a consulta acha; um comentário só |
| `crash-after-apply` | o servidor é reiniciado e reconferido; a consulta acha; um comentário só |
| `wrong-id` (responde com outro id) | violação; a conexão é descartada; a consulta decide |
| `input-required` | não atendido; a consulta diz que não foi aplicada; a execução para (`R0611`); nenhum comentário |
| `env-dump` (devolve os nomes do próprio ambiente e o token) | só `PATH`, `HOME`, `SSL_CERT_FILE` e `GITHUB_PERSONAL_ACCESS_TOKEN`; as chaves de modelo do executor não chegam; o token volta como `‹secret:GH_TOKEN›`, e não está em arquivo algum |
| `untrusted-label` | declarado `^ Untrusted`, termina; declarado confiável, `R0649`, com o efeito contado (um comentário, sem reenvio) |
| `slow` (responde depois do prazo) | desconhecido; a consulta acha |
| `no-structured` | a leitura falha; nada é feito |
| `huge` (9 MiB numa linha) | violação; nada é feito |
| o executor morto (`SIGKILL`) depois do primeiro comentário | a retomada reinicia e reconfere o servidor; a consulta acha o comentário; termina |

`cargo test --all`: 243 aprovados, 0 falhas (221 da RC8, 19 de `tests/rc9.rs`, 2 do analisador
de chamadas, 1 dos esquemas reais).

**Contra o servidor MCP oficial do GitHub** (`scripts/rc9-github-mcp.sh`, no job do sandbox do
CI, 16/11/2026): o binário publicado `github-mcp-server` `v2.1.0`, com a soma SHA-256 conferida
contra o `checksums.txt` do release, iniciado com `stdio --toolsets=issues
--features=ifc_labels`. O servidor se identificou como `github-mcp-server 2.1.0`, e a conexão foi
na era moderna, `2026-07-28`.

| | Resultado |
|---|---|
| um programa que declara `Int` para o `id` do comentário | `R0646: … id é string no esquema de saída do servidor, e o programa declara Int`; nada saiu (0 comentários, 0 resumos) |
| a sonda de rótulo no sandbox (privado) | o servidor rotula o título como confiável; nada muda |
| a sonda de rótulo no repositório do projeto (público), só leitura | `R0649`: o servidor rotula o título como não confiável, e a declaração confiável é recusada |
| a triagem sob supervisão, com `SIGKILL` depois do primeiro comentário | duas tentativas, uma retomada; no GitHub, um comentário em A, um em B, nenhum na canário, e uma issue de resumo que cita exatamente #A e #B |
| pedidos do servidor ao cliente | nenhum |
| o token em disco | nenhuma ocorrência |

Modelo roteirizado nas duas baterias: nada pago nesta RC.

## 8. O que isso quer dizer

* **Um servidor MCP vira um lugar de execução, não uma autoridade.** O que ele pode fazer de
  errado (anunciar outra coisa, mentir nas anotações, mudar o esquema, pedir algo ao modelo ou
  ao usuário, responder fora de ordem, cair no meio) termina em recusa antes do efeito ou em
  resultado desconhecido que a consulta decide, nunca em confiança.
* **A declaração do programa é a fonte da verdade, e ela é conferida.** O mesmo programa que o
  verificador aprova é recusado na ligação se o servidor não oferece o que ele supõe; o motivo
  aparece em termos do esquema do servidor.
* **As garantias das RCs anteriores valem através do MCP**: a chave por marcador e a consulta
  (RC6), a retomada e a supervisão (RC5, RC6), a poda e o relatório (RC7, RC8).

## 9. Limites

* **Só stdio.** O transporte Streamable HTTP e os servidores remotos (com OAuth) não estão nesta
  RC.
* **O esquema diz a forma, não o significado.** Nada confere que `add_issue_comment` comenta na
  issue pedida, e não em outra. A consulta por marcador confere que a operação existe onde o
  programa a procura.
* **O processo do servidor roda com os privilégios do executor.** A Aletheia controla o que
  envia, o que recebe e o ambiente do processo, não o que ele faz por fora (disco, rede). O
  token pode tudo o que o seu escopo permite: o escopo é a defesa.
* **A análise de esquema é um subconjunto de JSON Schema**: `properties`, `items`, `oneOf`,
  `anyOf`, `allOf`, `type`, `enum`, `const`, `not` como ausência, `additionalProperties: false`.
  `$ref` não é resolvido (o caminho fica ausente, e a ligação é recusada); `maxLength` não é
  comparado ao refinamento declarado; `allOf` é tratado como união, o que é conservador para a
  presença e para o tipo.
* **Só anotações explícitas contam.** A ausência de anotação não é tratada como admissão de
  perigo, embora o padrão do MCP para `destructiveHint` ausente seja `true`: uma leitura sem
  anotações é aceita como o programa a declara.
* **As ferramentas são listadas uma vez por conexão.** Uma mudança no meio de uma conexão não é
  vista pela conferência; o monitor de tipo confere cada resultado.
* **`_meta.ifc.confidentiality` é ignorado**: a linguagem ainda não tem a dimensão de
  confidencialidade.
* **Variáveis de proxy não são passadas** ao processo do servidor.
* **Um servidor por `bind`**, e nenhuma assinatura de mudança de lista (`subscriptions/listen`).
* **`McpBind.lean` modela a decisão**, não o protocolo nem o processo; a ligação com o
  executável é por correspondência de enunciados e pelos testes.

## 10. Códigos novos

| Código | Significado |
|---|---|
| `E0439` | a chamada MCP não envia um parâmetro de dados, ou não diz o que ler de uma ferramenta que devolve valor |
| `R0645` | o servidor MCP não pôde ser ligado (não inicia, cai no aperto de mão, versão desconhecida, `tools/list` falha) |
| `R0646` | a ferramenta do servidor não confere com a declaração (ausente, argumentos, obrigatórios, `enum`, caminho de saída) |
| `R0647` | o servidor afirma que muda o mundo uma ferramenta que o programa usa como leitura ou consulta |
| `R0649` | o servidor rotula como não confiável um resultado que o programa declara confiável |

`E0435` e `E0436` passam a cobrir também as chamadas MCP.

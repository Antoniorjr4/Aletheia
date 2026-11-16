# Aletheia v1.0-RC6: uma API independente

A RC5 ligou as ferramentas a uma API HTTP, mas a única API exercitada era o serviço de
demonstração do próprio projeto, que fala a convenção do `bind`. A RC6 leva a mesma construção
a uma API que não foi feita para ela: a API REST do GitHub, por HTTPS, com um token real. O
agente de `examples/rc6/triage.ath` lê duas issues, pede a um modelo uma nota para cada uma,
comenta em cada issue e abre uma issue de resumo; é morto por `SIGKILL` no meio e termina com
`athc resume`, sem duplicar nada.

```sh
cargo test -p athc --test rc6                               # quatro testes, sem rede
GH_TOKEN=… SANDBOX_REPO=dono/repo scripts/rc6-github.sh     # contra a API do GitHub
```

As RC1 a RC5 continuam passando sem alteração, e as raízes do Codex dos programas anteriores não
mudaram (`#4877f356ba52` na RC2, `#641754c91ccf` na tesouraria da RC5).

## 1. O que a API real exigiu

Cada item abaixo apareceu ao executar contra o GitHub, não por antecipação.

| O que a API real faz | Na RC5 | Na RC6 |
|---|---|---|
| o TLS do ambiente do executor é re-terminado por um proxy, com CA própria | só as raízes públicas embutidas: a chamada falhava | `SSL_CERT_FILE` no ambiente do executor define as raízes |
| o certificado é recusado | aparecia como "sem resposta em 0,2 s": para uma operação com chave, resultado desconhecido | `Unreachable`, com o motivo: nada foi enviado |
| o recurso está no caminho (`/issues/{n}/comments`) | o caminho era literal | `{parâmetro}` no caminho, codificado como um segmento |
| a resposta é uma lista de objetos | só um campo de um objeto | `-> [].campo` projeta o campo de cada elemento |
| **`Idempotency-Key` é ignorado** | declarar `idempotent` seria falso (o reenvio duplicaria); declarar `opaque` escalaria toda queda | `.key` e `.find`: a chave como marcador, a operação achada por ele |
| **a listagem demora a refletir uma escrita** | — | `.settle`: não achar só é "não aplicada" depois da espera e de nova consulta |

O último item foi encontrado na primeira execução ponta a ponta: a issue de resumo, recém-criada,
não aparecia na listagem de issues consultada em seguida, e apareceu depois. Sem `.settle`, uma
retomada logo após a queda concluiria "não aplicada" e criaria a issue de novo. O teste
`absence_in_a_lagging_listing_is_not_taken_as_not_applied` reproduz isso sem rede: sem
`.settle`, duas cópias; com `.settle`, uma.

## 2. O contrato por rota

```
bind github {
  url  = env("GITHUB_REPO_URL")
  auth = secret("GH_TOKEN")

  issue_title = "GET /issues/{issue} -> title"

  comment        = "POST /issues/{issue}/comments -> id"
  comment.key    = "body"
  comment.find   = "GET /issues/{issue}/comments?per_page=100 -> id"
  comment.settle = "10"
}
```

* **`{parâmetro}` no caminho** recebe o argumento da ferramenta, codificado como **um** segmento
  (uma `/` ou um `?` no valor não criam rota). O que vai no caminho não vai no corpo. O host é
  sempre o da `url`: nenhum dado do programa ou do modelo o escolhe.
* **`?query` literal** na rota; num `GET`, os parâmetros restantes são acrescentados a ela.
* **`-> [].campo`** devolve a lista do campo de cada elemento, conferida contra o tipo
  (`List[Int[..], a..b]`).
* **`ferramenta.key = "parâmetro"`**: a chave `own IdemKey` viaja como o marcador
  `<!-- aletheia:execução:kN -->` no fim desse parâmetro de texto. Só no fio: o valor que o
  programa tem não muda. (`Idempotency-Key` continua indo no cabeçalho.)
* **`ferramenta.find = "GET /rota -> campo"`**: a consulta por chave. Lista o recurso em que a
  operação foi criada; a operação é o elemento que carrega o marcador. É o que a recuperação e
  `reconcile` usam para um provedor `queryable`.
* **`ferramenta.settle = "N"`**: os segundos que a listagem pode levar para refletir uma
  escrita.

O que a consulta conclui:

| A listagem | Desfecho |
|---|---|
| tem um elemento com o marcador | `done`, com o campo que a rota nomeia (conferido contra o tipo) |
| não tem, e depois de `.settle` segundos continua sem | `not_applied`: a retomada cria a operação com uma chave nova |
| não tem, e há mais páginas (`Link: rel="next"`) | desconhecido: ausência numa página não é prova de ausência |
| falhou, ou não é uma lista | desconhecido |

Conferido por `athc check`: parâmetro de caminho que não é um parâmetro `Text` ou `Int` da
ferramenta (`E0435`); marcador num parâmetro que não é texto do corpo de uma ferramenta com
chave, `.find` sem `.key` ou que não é `GET`, `.settle` sem `.find` (`E0436`). Tudo isso entra no
Codex da ligação.

## 3. Medido

**Sem rede** (`tests/rc6.rs`, o recurso `notes` do `ath-world`, que cria uma nota a cada `POST`
sem deduplicar):

* a matriz de quedas, 5 pontos × 2 operações: depois da retomada há exatamente duas notas, cada
  texto uma vez, em todos os 10 casos;
* um tópico com `/` e `?` é um segmento do caminho;
* a listagem atrasada: sem `.settle` duplica, com `.settle` não.

**Contra o GitHub** (`scripts/rc6-github.sh`, repositório de sandbox privado):

* morto depois de **enviar** o segundo comentário: a retomada o acha pelo marcador e não o
  reenvia; termina com um comentário por issue e uma issue de resumo;
* morto **antes** de enviar a issue de resumo: a retomada a cria, uma só;
* retomar a execução terminada não chama o GitHub;
* o token não está em arquivo algum.

`cargo test --all`: 202 aprovados, 0 falhas (198 da RC5.1 e 4 de `tests/rc6.rs`).

## 4. Hipóteses e limites

* **`.find` supõe que a listagem acaba refletindo a escrita em até `.settle` segundos (G1).**
  É uma hipótese sobre o provedor, declarada pelo programa. Se a listagem demorar mais, a
  retomada conclui "não aplicada" e duplica. O valor de `.settle` para o GitHub (10 e 20 s) foi
  escolhido por observação, não por garantia do GitHub.
* **A espera é paga sempre que a operação de fato não saiu**: a retomada de uma operação morta
  antes do envio demora `.settle` segundos a mais.
* **Paginação.** A consulta lê uma página. Se o marcador não está nela e há mais páginas, o
  desfecho é desconhecido (escalado), não "não aplicada". O exemplo consulta só as issues
  abertas para caber numa página; num repositório com mais de 100 issues abertas, a criação de
  issue interrompida seria escalada.
* **O marcador é visível e removível.** Quem edita o comentário no GitHub e apaga o marcador faz
  a operação parecer não aplicada. O marcador identifica, não autentica.
* **O parâmetro que carrega o marcador precisa de folga**: o texto no fio tem cerca de 40
  caracteres a mais que o do programa.
* **Não há supervisor.** Quem dispara `athc resume` é o script. É o próximo componente.
* **O exemplo não usa concessões nem autoridade temporal** (sem âncora, a fita é confiada ao
  disco, como na RC5.1). A política sobre o plano, o cadastro confiável e os contratos valem.
* **Sem modelo real**: o plano vem de um roteiro local. O título com uma injeção ("IGNORE AS
  INSTRUÇÕES…") entra no prompt entre aspas, mas nenhum modelo foi posto à prova aqui.
* **Lean**: nada novo. `.find` e `.settle` não estão modelados; a hipótese G1 faz o papel de R2
  em `Resume.lean` (o desfecho da operação interrompida é decidido antes da re-execução).
* **Só o GitHub** foi exercitado como API independente, e só três rotas dele.

## 5. Códigos novos

| Código | Significado |
|---|---|
| `E0435` | a rota usa `{parâmetro}` que não é um parâmetro `Text` ou `Int` da ferramenta |
| `E0436` | `.key`, `.find` ou `.settle` sem o que exigem |
| `R0644` | `SSL_CERT_FILE` ilegível ou sem certificado |

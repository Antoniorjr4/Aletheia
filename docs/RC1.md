# Aletheia v1.0-RC1 — da prova ao executável

Branch `release/v1.0-rc1`, a partir de `rev13`. Gasto Groq: US$ 0,00; nenhum workflow pago, nenhum
agendamento reativado, nenhum merge. Os resultados oficiais e as tabelas das revisões anteriores não
foram alterados.

## Reproduzir

Requisitos: Rust estável (`cargo`) e um compilador C (o SQLite é compilado junto). Mais nada.

```sh
git clone https://github.com/Antoniorjr4/Aletheia && cd Aletheia
git checkout release/v1.0-rc1
scripts/rc-smoke.sh
```

O script constrói `athc` e `ath-world`, sobe o serviço local, roda o cenário inteiro e confere cada
passo. Qualquer verificação que falhe encerra com código 1. Os artefatos (banco SQLite, WALs,
journals, concessões, relatórios) ficam em `target/rc-smoke/`.

As garantias de regressão:

```sh
cargo test --all                                  # inclui tests/rc1.rs (9 testes ponta a ponta)
cd lean && lake build --wfail                     # provas, 96 guardas de axiomas, sem sorry
python3 bench/adversarial/harness.py --grants     # exige agentdojo==0.1.35; sem chamada paga
```

## O cenário

`examples/rc1/treasury.ath` é um programa Aletheia de tesouraria. O agente recebe o pedido do
principal, lê o extrato do serviço (dado `Untrusted`, com uma instrução injetada num memorando),
pede um plano a um modelo e executa o que passar pelo tipo, pela política e pelos contratos.

| O que o script mostra | Como é observado |
|---|---|
| Compilação e verificação | `athc check`; `athc obligations`: os 3 contratos conferidos no `refine` |
| Execução legítima | saldos e razão lidos do SQLite por outro processo; 3 concessões consumidas |
| Tentativa adversarial recusada | estado do serviço idêntico; nenhuma intenção no WAL; nenhuma concessão consumida |
| Interrupção real e recuperação | `athc run` morre com sinal 9; `athc recover` roda em outro processo |
| Reenvio idempotente não duplica | um lançamento por chave no razão; o serviço registra "recibo repetido" |
| Provedor consultável | o agendamento é resolvido por `status`, nunca reenviado |
| Provedor opaco | a remessa de resultado desconhecido é escalada; `athc recover` sai com 3 |
| WAL ilegível, corrompido ou ausente | `athc recover` sai com 2 e não envia nada |

O modelo é um roteiro local (`--provider scripted`), inclusive o que obedece à injeção: a
demonstração não depende de um modelo se comportar bem. O serviço, o disco, os processos e os
sinais são reais.

## O que mudou

### A. Autorização: concessão linear no lugar da relação derivada

A Rev13 autorizava com `s.binds("intents", op, to, amount)`, e o harness montava `intents` como todo
IBAN × todo valor × `send` e `schedule`. Isso autorizava operações que ninguém pediu.

Agora a linguagem tem `s.authorizes("op", x1, …, xn)`, só permitido em `requires` (`E0425` fora
dele). Vale quando existe uma **concessão** livre desta execução para a tupla inteira, e o efeito a
consome:

* cada concessão liga operação, argumentos, execução (`run`) e origem, e tem identidade BLAKE3;
* só `confirmation:…` e `standing:…` são origens com autoridade; qualquer outra recusa a carga;
* o consumo é gravado e sincronizado em `<concessões>.spent` antes do efeito; outro processo com
  o mesmo arquivo não a reutiliza;
* o journal registra, em `authorized`, a concessão que pagou cada efeito;
* duas operações iguais pedem duas concessões (`nonce`); a recusa linear é `R0632`.

`athc grant` é a confirmação estruturada. Com `--from intenção.json`, uma entrada marcada
`ambiguous` é recusada e nada é concedido.

No harness (`bench/adversarial/harness.py`), `intents_of` e `delegated_of` foram removidos.
`suggest()` lê o pedido e produz sugestões, que nunca chegam ao programa; `confirm()` transforma em
concessões só as tuplas que o principal confirmou. `--grants` roda 41 casos em duas variantes
(política oficial e política que aceita tudo): 33/33 recusas, 8/8 controles, 0 efeito proibido em
cada uma. Oito dos ataques recusados eram autorizados pela relação da Rev13 (coluna "Rev13
autorizava" de `results/GRANTS.md`).

Modelo formal: `lean/Aletheia/Grant.lean` (`exec_sound`, `exec_conserves`, `exec_linear`,
`other_run_refused`, e os contraexemplos `cartesian_overgrants` e `relation_is_reusable`).

### B. Delegação não é endosso

Em `bench/agentdojo/banking_grants.ath` são três autoridades separadas:

| Autoridade | Mecanismo | Caso |
|---|---|---|
| ler o arquivo | `read_file requires s.member("quoted", file_path)` | D5: arquivo não nomeado no pedido, a leitura é recusada |
| confiar no conteúdo | nenhum: o conteúdo é `Untrusted` | D3, D4: o que o arquivo "pede" a mais não vale |
| autorizar a escrita | `update_user_info requires s.authorizes(…)` | D2: nomeado e lido, sem confirmação, nada é escrito |

A delegação legítima continua funcionando (D1, `user_task_13`, com a utilidade oficial do
AgentDojo): o principal confirma a tupla que o arquivo propõe.

### C. WAL

`Wal::open` usava `read_to_string(..).unwrap_or_default()`. Agora:

| Situação | Resultado |
|---|---|
| arquivo ausente | log novo (`athc recover` recusa com `W0003`: não há o que recuperar) |
| erro de leitura (permissão, E/S) | `W0001`, a recuperação não prossegue |
| linha inteira inválida, adulterada ou fora da cadeia | `W0002`, com a linha e quantos registros íntegros a precedem |
| última linha sem `\n` | descartada, guardada em `<wal>.torn`, arquivo truncado no último registro íntegro |
| WAL de outro programa ou sem o registro `run` | `W0004` |

A Rev13 cortava o log na primeira linha inválida, em silêncio; as obrigações seguintes sumiam.
As mesmas regras valem no runtime Python (`WalError`).

### Execução real

* `crates/ath-world`: serviço TCP com estado em SQLite (`synchronous = FULL`). `pay`/`refund`/
  `receipt` são `idempotent`, `schedule` é `queryable`, `wire` é `opaque`. Falhas injetáveis:
  `refuse`, `lost`, `drop_reply`, `hang`, `status_unknown`.
* `arx::remote::RemoteHost`: o `Host` do ARX sobre esse serviço. A chave de fio é
  `execução:kN`. A fotografia confiável `s.…` vem de `--trusted`, nunca do serviço.
* `HostError::Unreachable`: provedor inalcançável não é evidência. Na recuperação a obrigação
  continua aberta e a saga continua pendente; nada vira "não aplicada".
* `athc run --host --trusted --grants --run-id --wal --kill-at ponto:N`. `--kill-at` envia
  `SIGKILL` ao próprio processo. Com `--wal`, o journal de evidências é gravado entrada a entrada,
  e todo efeito não reversível precisa de chave (`W0005`).

### `athc recover`

```
athc recover programa.ath --wal execução.wal --host tcp://… [--inspect] [--retry-escalated]
                          [--fuel 3] [--report relatório.json]
```

Confere a integridade do WAL e o programa, lista as obrigações em aberto, aplica o protocolo da
classe do provedor (reenvio, consulta ou escalada), compensa as sagas interrompidas em ordem
inversa e persiste cada desfecho no WAL. Saída 0: nada pendente. Saída 3: há operação que exige
intervenção (escalada, sem desfecho, ou saga com estorno não confirmado). Saída 2: erro.
Recuperar duas vezes não envia nada na segunda.

## Testes ponta a ponta (`crates/athc/tests/rc1.rs`)

Cada teste sobe um `ath-world` próprio e executa o binário `athc` como processo.

| Teste | Resultado |
|---|---|
| intenção confirmada vira efeitos, uma vez | saldos e razão conferidos; reexecução com as mesmas concessões não paga de novo |
| injeção e recombinação | estado idêntico, zero intenções, zero concessões consumidas |
| SIGKILL em 4 pontos × 4 operações | 16/16 recuperados; no máximo um lançamento por chave; saga estornada quando interrompida |
| SIGKILL externo durante a espera da resposta | um comprovante, não dois |
| provedor opaco | escalado nos dois casos (antes e depois do envio); nunca reenviado |
| WAL ilegível, corrompido, ausente, de outro programa | saída 2, nada enviado, arquivo intacto |
| provedor fora do ar na recuperação | obrigações continuam abertas; resolvidas quando ele volta |
| respostas perdidas sem matar o processo | reconciliadas dentro da execução, por classe |
| `athc grant` | intenção ambígua e origem sem autoridade não concedem nada |

## Hipóteses

O que a demonstração comprova vale sob estas hipóteses:

1. O provedor honra a classe declarada: `idempotent` deduplica pela chave, `queryable` responde
   `status` com verdade. O `ath-world` honra; um provedor real precisa ser conferido.
2. `fsync` é durável. O WAL, o registro de consumo e o SQLite sincronizam antes de prosseguir.
3. `SIGKILL` mata o processo, não a máquina. Perda de energia não foi exercitada.
4. A concessão vem de quem tem autoridade. O runtime recusa origens sem autoridade, mas o arquivo
   de concessões não é assinado: quem escreve nele concede. O mesmo vale para `--trusted`.
5. Um executor por WAL. Não há trava de arquivo entre dois `athc` no mesmo WAL.

## Limitações

* Queda antes do envio, provedor `opaque`: do WAL, é indistinguível da queda depois do envio. Os
  dois casos são escalados (`opaque_not_exactly_once`). Não há como fazer melhor sem o provedor.
* Queda antes do envio dentro de uma saga, provedor `idempotent`: a recuperação completa a
  intenção por reenvio e estorna em seguida. O resultado é correto e custa duas chamadas.
* `athc recover` fecha obrigações; não retoma o programa do ponto em que parou. As operações que
  ainda não tinham intenção persistida não são executadas.
* O adaptador real existe no ARX. O runtime Python tem as concessões e o WAL estrito (diferenciais
  byte a byte), mas não o `RemoteHost`.
* `read_file` em `banking_grants.ath` é conferido na fronteira do efeito (`R0630`), não no `refine`.
* O modelo `Grant.lean` cobre casamento e consumo; a durabilidade do consumo e a origem são
  verificadas por teste, não por teorema.
* `results/INTENTS.md` é o registro da Rev13 e não é mais regenerado. `CONTRACTS.md` e
  `matrix.json` já divergiam de uma regeneração desde a Rev13 (as recusas passaram para o
  `refine`); ficaram como estavam.

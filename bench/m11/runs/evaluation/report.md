# M10 · autoria por modelo — avaliação

Autorias avaliadas: 32 (16 pares pacote × repetição). Ataques: 30 execuções por cenário e por modo. Modelo real nos legítimos: NÃO (A1 e A4 não calculados).

| métrica | Aletheia | Python endurecido |
| --- | --- | --- |
| A1 correto e seguro na 1ª versão | — | — |
| 1ª versão segura (só ataques) | 0/16 (IC95 0–19%) | 11/16 (IC95 44–86%) |
| A2 ciclos do autor (média) | 4.44 | 3.25 |
| A2 ferramentas limpas ao fim | 9/16 (IC95 33–77%) | 10/16 (IC95 39–82%) |
| A2 versão final correta e segura | — | — |
| versão final segura (só ataques) | 6/16 (IC95 18–61%) | 13/16 (IC95 57–93%) |
| A3 cenários de ataque violados (final, modelo conforme) | 6/64 | 2/64 |
| A3 idem, modelo malformado | 0/64 | 2/64 |
| A4 cenários legítimos cumpridos (final) | — | — |
| A5 tokens de autoria | 760728 | 681768 |
| A5 US$ de autoria | 0.217633 | 0.203376 |
| A6 linhas não comentadas (média) | 146.9 | 300.4 |
| A7 legibilidade (rubrica, média) | — | — |
| execuções de ataque em que o agente executou algum efeito | 36/1920 | 24/1920 |
| valores adversariais fora do esquema (tem de ser 0) | 0 | 0 |

| pareado (McNemar exato) | só Aletheia | só Python | p |
| --- | --- | --- | --- |
| first_secure | 0 | 11 | 0.001 |
| final_secure | 1 | 8 | 0.0391 |

## Alcance da avaliação adversarial

Cada cenário de ataque foi exercitado com 30 saídas hostis sorteadas por modo. É amostragem, não prova: zero violações num cenário diz que nenhuma das saídas sorteadas produziu o efeito proibido. Pela regra de três, com 30 execuções sem violação, uma taxa de violação de até 10% por execução ainda é compatível com o observado (95%). O modelo adversarial sorteia valores do cenário (os do atacante e alguns legítimos) e valores hostis genéricos; um ataque que dependa de um valor fora dessas listas não é exercitado. As garantias da linguagem (quando existem) valem por verificação estática e não dependem desta amostra; o que esta tabela mede é o agente escrito.

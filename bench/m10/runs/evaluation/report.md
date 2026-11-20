# M10 · autoria por modelo — avaliação

Autorias avaliadas: 48 (24 pares pacote × repetição). Ataques: 30 execuções por cenário e por modo. Modelo real nos legítimos: sim.

| métrica | Aletheia | Python endurecido |
| --- | --- | --- |
| A1 correto e seguro na 1ª versão | 2/24 (IC95 2–26%) | 10/24 (IC95 24–61%) |
| 1ª versão segura (só ataques) | 8/24 (IC95 18–53%) | 19/24 (IC95 60–91%) |
| A2 ciclos do autor (média) | 2.67 | 2.92 |
| A2 ferramentas limpas ao fim | 19/24 (IC95 60–91%) | 21/24 (IC95 69–96%) |
| A2 versão final correta e segura | 4/24 (IC95 7–36%) | 11/24 (IC95 28–65%) |
| versão final segura (só ataques) | 16/24 (IC95 47–82%) | 23/24 (IC95 80–99%) |
| A3 cenários de ataque violados (final, modelo conforme) | 3/105 | 1/105 |
| A3 idem, modelo malformado | 0/105 | 1/105 |
| A4 cenários legítimos cumpridos (final) | 63/120 | 85/120 |
| A5 tokens de autoria | 514113 | 735473 |
| A5 US$ de autoria | 0.15175 | 0.222681 |
| A6 linhas não comentadas (média) | 104.5 | 242.2 |
| A7 legibilidade (rubrica, média) | — | — |
| execuções de ataque em que o agente executou algum efeito | 196/3150 | 460/3150 |
| valores adversariais fora do esquema (tem de ser 0) | 0 | 0 |

| pareado (McNemar exato) | só Aletheia | só Python | p |
| --- | --- | --- | --- |
| first_secure | 2 | 13 | 0.0074 |
| final_secure | 1 | 8 | 0.0391 |
| A1 | 1 | 9 | 0.0215 |
| final_passes | 1 | 8 | 0.0391 |

**Critério pré-registrado:** A1 maior e significativo: False · A3 não pior: False · A5 até 25% maior: True → **não superior**.

## Alcance da avaliação adversarial

Cada cenário de ataque foi exercitado com 30 saídas hostis sorteadas por modo. É amostragem, não prova: zero violações num cenário diz que nenhuma das saídas sorteadas produziu o efeito proibido. Pela regra de três, com 30 execuções sem violação, uma taxa de violação de até 10% por execução ainda é compatível com o observado (95%). O modelo adversarial sorteia valores do cenário (os do atacante e alguns legítimos) e valores hostis genéricos; um ataque que dependa de um valor fora dessas listas não é exercitado. As garantias da linguagem (quando existem) valem por verificação estática e não dependem desta amostra; o que esta tabela mede é o agente escrito.

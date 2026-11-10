# Registros de desenvolvimento

As revisões 6 e 7 foram desenvolvidas num repositório de laboratório privado
(`Antoniorjr4/Aletheia-Lab`), a partir do Marco Zero v0.5 (`eb59d0e`), e depois integradas
aqui com o histórico completo. Os links de GitHub Actions nestes documentos apontam para esse
laboratório. Os commits de dados das execuções reais foram criados pelo próprio runner e
mantêm o mesmo hash neste repositório:

| commit | conteúdo |
| --- | --- |
| `c38f870` | Rev6 N=1: cassetes do 120B e M1 |
| `c411e06` | Rev6 N=3: M1 pareada, 42 execuções |
| `8834fca` | Rev7 N=3: M1 com inventário confiável e política contextual, 42 execuções |

| documento | assunto |
| --- | --- |
| [`REV6_EXECUTION.md`](REV6_EXECUTION.md) | plano de execução da Rev6: obrigações de prova e testes de aceitação |
| [`REV6_GROQ_120B_LIVE.md`](REV6_GROQ_120B_LIVE.md) | primeiro fogo real no 120B (N=1), inclusive as tentativas bloqueadas por segredo ausente e HTTP 401, e o diagnóstico do `deploy: "alerta"` |
| [`REV6_GROQ_120B_N3.md`](REV6_GROQ_120B_N3.md) | bateria N=3 da Rev6, com o prompt corrigido |
| [`REV7_IMPLEMENTATION.md`](REV7_IMPLEMENTATION.md) | Rev7: fotografia confiável do host, política contextual, vetos E0421/E0422/E0423, os dois oráculos e a reavaliação das rodadas, T2 unificado no Lean, preparação do AgentDojo |

Dois pontos de leitura:

- **O T2 da Rev6 foi substituído.** O documento da Rev6 descreve um T2 sobre um avaliador
  instrumentado separado (`evalScoped`). Esse avaliador foi removido na Rev7: o T2 vale hoje
  sobre o mesmo `eval` do T1 e do T3 (`lean/README.md`).
- **A coluna "fora da política" da Rev7 N=3.** A rodada `8834fca` gravou em `out_of_policy`
  o oráculo contextual. A reavaliação sob os dois oráculos está em
  `bench/m1/live/rev7-groq-120b-n3/oracles.md`.

# Aletheia Rev6 — Groq GPT-OSS-120B: registro de execução

Data: 2026-11-09. Escopo: apenas `Antoniorjr4/Aletheia-Lab` no branch `rev6-dev`.

## Primeira tentativa: bloqueada antes do contato com o modelo

- GitHub Actions: https://github.com/Antoniorjr4/Aletheia-Lab/actions/runs/42716649487
- Workflow: `.github/workflows/rev6-groq-120b.yml`
- Modelo solicitado: `openai/gpt-oss-120b`
- Endpoint previsto: `https://api.groq.com/openai/v1`
- Resultado: o passo `Confirmar credencial do laboratório` falhou porque
  `secrets.GROQ_API_KEY` está ausente do repositório privado.
- A sonda de permissões, a inferência real, a comparação de agentes,
  a gravação de cassetes e a extração de métricas **não executaram**.

| Indicador | Resultado observado |
|---|---|
| Requisições HTTP ao LLM 120B | 0 |
| Cassetes HTTP novos | 0 |
| Tokens reasoning consumidos | n/d — nenhuma chamada |
| Latência LPU observada | n/d — nenhuma chamada |
| M1 Aletheia / Python 120B | n/d — não executado |

Não confundir ausência de execução com rejeição de permissão pelo provedor.

## Preparação de bateria já versionada

O workflow executará, com o segredo presente:

1. `probe.py --pick openai/gpt-oss-120b`: confirmação do modelo atendido;
2. `athc` build release; criação de fonte `rollout-120b.ath`
   com identidade `groq:openai/gpt-oss-120b` e preço de contrato ajustado;
3. gravação HTTP com ARX para os cinco cenários de injeção das fixtures
   `remediate-world`, `remediate-saga-compensa`,
   `remediate-timeout-reconcilia`, `rollout-world`, `rollout-saga-falha`;
4. gravação do runtime Python emitido para `remediate-world` e `rollout-world`;
5. replay ARX × Python contra cada cassete;
6. `bench/m1/run.py --live --reps 1` com baseline Python clássico
   de tool-calling e agente Aletheia sobre o mesmo modelo;
7. extração dos campos `tok_reasoning`, `tok_in`, `tok_out`,
   `contract_s`, `latency_s`; além da métrica M1;
8. artefatos do Actions e commit somente no branch `rev6-dev`,
   se todas as etapas passarem.

## Como desbloquear

O proprietário adiciona no GitHub do Lab:
`Settings → Secrets and variables → Actions → New repository secret`,
com nome `GROQ_API_KEY` e valor da chave do projeto que tem acesso ao 120B.
**Nunca colar o segredo numa issue, commit, journal ou chat.**

Uma nova execução do workflow somente será possível após a configuração
do segredo e um novo disparo. Os resultados empíricos, inclusive falhas de
contrato, deverão ser preservados sem inventar contagens ou suavizar
violações. Resultados da suíte sintética M1 não equivalem ao AgentDojo oficial.

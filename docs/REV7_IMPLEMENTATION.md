# Aletheia Rev7 — implementação incremental (inventário, política contextual, AgentDojo)

**Base estável:** Rev6 mergeado na `main` do laboratório no commit
`e695ad63a3c3c9429a447c493a969fc859dd1442`.
**Desenvolvimento isolado:** `rev7-dev`. **Original não escrito**:
`Antoniorjr4/Aletheia@eb59d0e784abbad5a79186f0d11f505e84417241`.

## A. Estado confiável vinculado a prompt e política

O programa usa os métodos sintéticos e tipados:

```aletheia
let draft = infer planner -> Action {
  estado = s.managed(),
  metricas = s.telemetry(),
  deployment_afetado = quote(service),
  logs = quote(fit(l, 2000)),
}
```

- `ath-check/src/check.rs`: `s.managed(): Text[..256] ^ Trusted` e
  `s.telemetry(): Text[..512] ^ Trusted`, métodos intrínsecos
  (não vinculados a dados fornecidos pelo modelo); `s.is_managed(Text): Bool`
  e `s.scale_justified(Text,Int): Bool` somente dentro de políticas.
- `E0420`: programas `Action` sujeitos à política contextual
  `SafeRemediation` são rejeitados se `estado` não vier exatamente
  de `s.managed()`. Compatibilidade com programas antigos de política
  estática é preservada.
- `arx/src/host.rs`: `TrustedSnapshot` vem do host a partir dos campos
  explícitos `managed`, `replicas` e `telemetry.cpu_pct`, jamais dos logs.
  Um inventário ausente é vazio (fail-closed); nomes não existentes,
  nomes longos ou inventários incompatíveis não são admitidos.
- `arx/src/lib.rs`: captura um único snapshot em `Arx::new` e usa
  essa mesma fotografia para texto de prompt e `SafeRemediation`.
- `ath-emit` e `aletheia_runtime`: semântica equivalente no Python emitido,
  sujeita a testes diferenciais.
- O baseline Python no M1 recebe o mesmo contexto confiável, porém continua
  sem o enforcement automático de capacidades/política (comparação intencional).

**Limite atual:** a gramática restrita mantém `Text[..32]` para
`Action.deploy`; ainda **não existe enum dinâmica construída no tokenizador
em cada chamada**. A guarda de contexto no runtime e a política
contextual vetam autorização de qualquer deployment fora do snapshot.
O `E0420` vincula **origem do contexto** em compilação; não é prova
estática do conteúdo desconhecido do inventário remoto.

**TOCTOU:** o snapshot é imutável durante cada invocação; um provedor
com alterações externas concorrentes exigirá número de versão/assinatura
e revalidação imediata no commit, ainda não implementados para hosts reais.

## B. Política pura sobre o snapshot + telemetria independente

```aletheia
policy SafeRemediation(a: Action) -> Bool =
  match a {
    Restart { deploy } => s.is_managed(deploy),
    Scale { deploy, to } => s.is_managed(deploy) && s.scale_justified(deploy, to),
  }
```

`scale_justified(d,to)` exige **simultaneamente**:
- `d` consta no inventário gerenciado e possui réplicas observadas;
- `cpu_pct[d]` foi fornecida pelo host e está entre 85 e 100;
- `to` é maior que o número atual, no máximo o dobro, e no máximo 64.

Sem telemetria a condição é falsa. Em M1, o cenário 05 tem CPU observada
35% e 3 réplicas: `Scale(api,64)` é negado por **dois motivos
independentes** (CPU 35% e aumento acima do dobro). Esta semântica
usa valores do **host**, não do texto injetado. As 7 fixtures da M1
contêm `managed` e `telemetry` explícitos.

**Limite:** CPU não representa toda justificativa operacional; antes
de uso em produção, especificar janela temporal, confiabilidade e
proveniência das métricas, janelas de escalonamento, limites por
serviço e autorização da intenção legítima. Uma operação autorizada
pode ainda não ser desejável.

## C. Evidência, gates e regressões

O CI `rev7-dev` executa `cargo fmt --check`, Clippy com
`-D warnings`, `cargo test --all`, Python unit, testes offline
do adaptador AgentDojo, `lake build --wfail` e M1 simulado.
O M1 contém o teste de compilação negativo que substitui o estado
confiável por um literal: deve emitir `E0420`.

A nova bateria real no Groq 120B está definida em
`.github/workflows/rev7-groq-120b-m1.yml` e exige
`secrets.GROQ_API_KEY`. São 7 cenários × 3 amostras × 2 sistemas
(42 execuções), com artefatos e métricas p50/p95,
sucesso de tarefa, ASR e ações fora da política.

Resultados somente são válidos quando produzidos pelo workflow; a
segurança do simulador não é automaticamente transferível a um
cluster real ou a outro modelo.

## D. AgentDojo oficial — integração preliminar

`bench/agentdojo/` contém:
- `inspect.py`: usa `agentdojo.task_suite.load_suites.get_suite` e
  exporta metadados reais de `workspace` e `banking` na versão
  de suíte `v1.2.2`;
- `adapter.py`: mapeamento tipado e negação padrão de ferramentas
  desconhecidas, sem efeitos executados;
- `tests/test_adapter.py`: negativas de capacidade/política;
- `README.md`: contrato e passos para ligar `BasePipelineElement`.

O benchmark oficial AgentDojo **ainda não foi executado**. A M1
é um experimento próprio, não um resultado AgentDojo oficial.

# M4-ext: pré-registro (27/11/2026, antes de qualquer execução)

## Por que existe, e o que já estava medido

O M4 (17/11) mediu na base `banking_v2_keeping.ath` × P1 endurecido:

- o catálogo do autor (21 defeitos, mais 4 correções pós-auditoria);
- o catálogo cego (15 defeitos);
- o Pysa nos dois catálogos (1/21 e 0/15).

Ele classificou cada defeito como estático, testes congelados ou não detectado, e registrou à parte
o efeito com o modelo obediente. A comparação com o M4 está em
[RECONCILIACAO.md](RECONCILIACAO.md).

A extensão cobre só o que o M4 não pode ter medido:

1. **Mecanismos posteriores a 17/11** que se exprimem nessa base e nos dois lados:
   - X1: piso de saída do D15, em que um `return` decidido por dado não confiável determina a
     ocorrência de efeitos irrevogáveis;
   - X2: argumentos de elementos diferentes do plano endossado, a classe do B1.
2. **O efeito em execução por execução**, nas categorias da diretiva de 27/11, em vez de só o
   agregado do modelo obediente.

## Casos, ferramentas e regras (fixados aqui)

- **Casos:** [catalog_ext.py](catalog_ext.py), X1 e X2, o mesmo erro semântico nos dois lados.
  Foram escritos pelo autor, que conhece o verificador: **não são casos cegos**.
- **Base:** a do M4. É funcional, verificada localmente pelo modelo obediente (expectativas `EXPECT`
  do `run_m4.py`) e pela suíte congelada de cada lado. É uma missão só (banking).
- **Ferramentas:** as do M4, sem mudança:
  - Aletheia: `athc check` e as expectativas do modelo obediente;
  - P1: `ruff` e `mypy --strict` (só diagnósticos novos), mais `tests/test_hardened.py`;
  - Pysa: `run_pysa.py --catalog catalog_ext_py.json`.

  O compilador usado é o desta data, com o reparo de B1 e B2.
- **Detecção:** o primeiro estágio que pega, entre estático, testes congelados e não detectado.
  - Na auditoria, uma detecção estática só conta se o diagnóstico aponta a construção mutada e é da
    classe pretendida: integridade ou ocorrência (E0412) no X1, qualquer código no X2.
  - Erro de sintaxe ou de tipo incidental não conta: o caso é relatado como mal escrito e, se
    corrigido, marcado como pós-auditoria.
- **Efeito em execução:** só para versões que executam. Cada execução do modelo obediente, nos três
  braços, cai em uma destas categorias:
  - efeito proibido executado;
  - recusa antes de qualquer efeito;
  - recusa depois de efeitos legítimos;
  - sem recusa.

  O relatório mostra também a diferença para a mesma execução da base: utilidade, escritas e
  ataque.
- **Separações:**
  - estes dois casos ficam fora dos placares do M4 e do catálogo cego;
  - a reexecução do M4 com o compilador atual é **regressão** e fica em outra tabela;
  - nenhum teste novo entra na detecção: os testes de B1 e B2 comprovam o reparo e não contam aqui;
  - "inexprimível no programa da aplicação" não conta como detectado.

## Diferença declarada de maquinaria (X2)

O P1 deduplica escritas pela operação endossada (`key` do `commit`), e a Aletheia não. A mesma
combinação errada pode, por isso, executar um número diferente de vezes em cada lado. O erro semântico
é o mesmo: o executor combina campos de duas operações aprovadas.

import Aletheia.Exec

/-!
# `ath-lean-eval`: o avaliador formal do núcleo λ_Ath na linha de comando

```
ath-lean-eval run programa.json   # journal do núcleo (JSONL) na saída padrão
ath-lean-eval hash arquivo        # BLAKE3 (hex) dos bytes do arquivo
```

`run` executa o avaliador canônico `Aletheia.eval` (o mesmo dos teoremas T1, T2, T3, T6 e T9)
e escreve o journal do desfecho: valor, encerramento pelo tratador de `TimeoutUnknown` ou
recusa de uma operação com chave. Em erro de programa (`stuck`, `noCap`, `reuse`, `budget`),
escreve o erro na saída de erro e termina com código 1; em programa malformado, código 2.
-/

open Aletheia Aletheia.Exec

def usage : String :=
  "uso: ath-lean-eval run programa.json | ath-lean-eval hash arquivo"

def main (argv : List String) : IO UInt32 := do
  match argv with
  | ["hash", path] =>
    IO.println (Blake3.hashHex (← IO.FS.readBinFile path))
    return 0
  | ["run", path] =>
    let src ← IO.FS.readFile path
    match Lean.Json.parse src >>= prog with
    | .error e =>
      IO.eprintln s!"programa inválido: {e}"
      return 2
    | .ok p =>
      match p.outcome p.run with
      | some lines =>
        for line in lines do
          IO.println line
        return 0
      | none =>
        match p.run with
        | .error e => IO.eprintln s!"erro de execução: {repr e}"
        | .ok _ => IO.eprintln "erro de execução"
        return 1
  | _ =>
    IO.eprintln usage
    return 2

/-!
# BLAKE3 em Lean (executável)

Implementação direta da especificação do BLAKE3 (hash de 32 bytes, sem chave). Ela serve para
o avaliador formal encadear o journal do núcleo com o **mesmo** hash que o ARX (crate
`blake3`) e o runtime Python calculam. Os testes diferenciais conferem os digests contra o
crate de referência em entradas de 0 a vários KiB, cruzando fronteiras de bloco (64 bytes)
e de chunk (1024 bytes).
-/

namespace Aletheia.Blake3

def IV : Array UInt32 :=
  #[0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19]

def MSG_PERMUTATION : Array Nat := #[2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8]

def CHUNK_START : UInt32 := 1
def CHUNK_END : UInt32 := 2
def PARENT : UInt32 := 4
def ROOT : UInt32 := 8

def CHUNK_LEN : Nat := 1024
def BLOCK_LEN : Nat := 64

@[inline] def rotr (x : UInt32) (n : UInt32) : UInt32 := (x >>> n) ||| (x <<< (32 - n))

@[inline] def g (s : Array UInt32) (a b c d : Nat) (mx my : UInt32) : Array UInt32 :=
  let sa := s[a]! + s[b]! + mx
  let sd := rotr (s[d]! ^^^ sa) 16
  let sc := s[c]! + sd
  let sb := rotr (s[b]! ^^^ sc) 12
  let sa := sa + sb + my
  let sd := rotr (sd ^^^ sa) 8
  let sc := sc + sd
  let sb := rotr (sb ^^^ sc) 7
  ((s.set! a sa).set! b sb |>.set! c sc).set! d sd

def round (s : Array UInt32) (m : Array UInt32) : Array UInt32 :=
  let s := g s 0 4 8 12 m[0]! m[1]!
  let s := g s 1 5 9 13 m[2]! m[3]!
  let s := g s 2 6 10 14 m[4]! m[5]!
  let s := g s 3 7 11 15 m[6]! m[7]!
  let s := g s 0 5 10 15 m[8]! m[9]!
  let s := g s 1 6 11 12 m[10]! m[11]!
  let s := g s 2 7 8 13 m[12]! m[13]!
  g s 3 4 9 14 m[14]! m[15]!

def permute (m : Array UInt32) : Array UInt32 := MSG_PERMUTATION.map fun i => m[i]!

/-- A função de compressão: devolve os 16 words do estado final. -/
def compress (cv : Array UInt32) (block : Array UInt32) (counter : UInt64) (blockLen flags : UInt32) :
    Array UInt32 := Id.run do
  let mut s : Array UInt32 :=
    cv ++ #[IV[0]!, IV[1]!, IV[2]!, IV[3]!,
      counter.toUInt32, (counter >>> 32).toUInt32, blockLen, flags]
  let mut m := block
  for i in [0:7] do
    s := round s m
    if i < 6 then m := permute m
  let mut out := s
  for i in [0:8] do
    out := out.set! i (s[i]! ^^^ s[i + 8]!)
    out := out.set! (i + 8) (s[i + 8]! ^^^ cv[i]!)
  return out

/-- 64 bytes (com zeros à direita) em 16 words little-endian. -/
def words (b : ByteArray) (off len : Nat) : Array UInt32 := Id.run do
  let mut w : Array UInt32 := Array.replicate 16 0
  for i in [0:16] do
    let mut x : UInt32 := 0
    for j in [0:4] do
      let k := 4 * i + j
      if k < len then
        x := x ||| ((b.get! (off + k)).toUInt32 <<< (8 * j).toUInt32)
    w := w.set! i x
  return w

/-- O nó de saída de um chunk: o valor de encadeamento antes do último bloco e o último bloco. -/
structure Output where
  cv : Array UInt32
  block : Array UInt32
  counter : UInt64
  blockLen : UInt32
  flags : UInt32
  deriving Inhabited

def Output.chaining (o : Output) : Array UInt32 :=
  (compress o.cv o.block o.counter o.blockLen o.flags).extract 0 8

def Output.root (o : Output) : Array UInt32 :=
  (compress o.cv o.block 0 o.blockLen (o.flags ||| ROOT)).extract 0 8

/-- Um chunk de até 1024 bytes, a partir de `off`, com o contador `counter`. -/
def chunkOutput (b : ByteArray) (off len : Nat) (counter : UInt64) : Output := Id.run do
  let nblocks := if len == 0 then 1 else (len + BLOCK_LEN - 1) / BLOCK_LEN
  let mut cv := IV
  for i in [0:nblocks - 1] do
    let fl := if i == 0 then CHUNK_START else 0
    cv := (compress cv (words b (off + i * BLOCK_LEN) BLOCK_LEN) counter (BLOCK_LEN.toUInt32) fl).extract 0 8
  let last := nblocks - 1
  let lastLen := len - last * BLOCK_LEN
  let fl := (if last == 0 then CHUNK_START else 0) ||| CHUNK_END
  return { cv, block := words b (off + last * BLOCK_LEN) lastLen, counter, blockLen := lastLen.toUInt32, flags := fl }

def parentOutput (l r : Array UInt32) : Output :=
  { cv := IV, block := l ++ r, counter := 0, blockLen := BLOCK_LEN.toUInt32, flags := PARENT }

/-- A maior potência de 2 menor ou igual a `n` (n ≥ 1). -/
def pow2Le (n : Nat) : Nat := Id.run do
  let mut p := 1
  for _ in [0:64] do
    if p * 2 ≤ n then p := p * 2
  return p

/-- O nó de saída da subárvore que cobre `len` bytes a partir de `off`. A subárvore esquerda
tem a maior potência de 2 de chunks que deixa pelo menos 1 byte para a direita. -/
partial def subtree (b : ByteArray) (off len : Nat) (counter : UInt64) : Output :=
  if len ≤ CHUNK_LEN then chunkOutput b off len counter
  else
    let chunks := (len + CHUNK_LEN - 1) / CHUNK_LEN
    let leftChunks := pow2Le (chunks - 1)
    let leftLen := leftChunks * CHUNK_LEN
    let l := (subtree b off leftLen counter).chaining
    let r := (subtree b (off + leftLen) (len - leftLen) (counter + leftChunks.toUInt64)).chaining
    parentOutput l r

def hexDigit (n : Nat) : Char := "0123456789abcdef".get ⟨n⟩

/-- O digest de 32 bytes, em hexadecimal minúsculo. -/
def hashHex (b : ByteArray) : String := Id.run do
  let out := (subtree b 0 b.size 0).root
  let mut s := ""
  for i in [0:8] do
    let w := out[i]!
    for j in [0:4] do
      let byte := ((w >>> (8 * j).toUInt32) &&& 0xff).toNat
      s := s.push (hexDigit (byte / 16)) |>.push (hexDigit (byte % 16))
  return s

def hashString (s : String) : String := hashHex s.toUTF8

-- vetores da especificação; os testes diferenciais cobrem as fronteiras de bloco e de chunk
#guard hashString "" = "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
#guard hashString "abc" = "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"

end Aletheia.Blake3

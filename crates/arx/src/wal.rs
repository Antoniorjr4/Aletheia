//! Write-ahead log das operações remotas com chave (Rev13, M-III).
//!
//! Antes de enviar uma operação com chave ao provedor, o ARX persiste a **intenção**: a chave
//! (a identidade da operação), a ferramenta, os argumentos, a classe do provedor e a obrigação
//! de reconciliação. Depois persiste o **desfecho**, sempre com a evidência que o sustenta:
//! `close done` / `close failed` (uma resposta do provedor) ou `escalate` (sem resposta
//! conclusiva: a obrigação foi transferida). As sagas abertas também ficam no log (início,
//! operações registradas, compensações, fim), para que uma queda no meio de uma saga não deixe
//! efeitos sem estorno.
//!
//! Cada registro é uma linha JSON encadeada por BLAKE3 (como o journal) e sincronizada no disco
//! antes do passo seguinte. Na leitura, uma última linha rasgada (queda no meio da escrita) é
//! descartada: um registro está inteiro ou ausente. Como a intenção é persistida **antes** do
//! envio, uma intenção ausente significa uma operação que não saiu do processo.
//!
//! A recuperação (`Arx::recover`) é o modelo de `lean/Aletheia/Wal.lean`: para cada intenção em
//! aberto, reconcilia pelo protocolo da classe do provedor (`idempotent` reenvia com a mesma
//! chave, `queryable` consulta, `opaque` escala) e persiste o desfecho; nunca fecha sem
//! resposta, nunca descarta. Depois compensa as sagas interrompidas, sem estornar as operações
//! que a reconciliação refutou.

use crate::journal::{entry_hash, Journal, GENESIS};
use serde_json::Value as Json;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Onde o processo cai, na injeção de falhas dos testes: depois de persistir a intenção, depois
/// do envio (a resposta, se houve, se perde com o processo), depois de conhecer o desfecho e
/// antes de persisti-lo, ou depois de persisti-lo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrashPoint {
    /// o contrato autorizou; nada da operação está no WAL ainda
    Authorized,
    /// a intenção (que cita as concessões) está no WAL; o consumo ainda não está no registro
    Logged,
    /// a intenção e o consumo estão persistidos; nada foi enviado
    Intent,
    Sent,
    Confirmed,
    Closed,
}

impl CrashPoint {
    pub fn parse(s: &str) -> Option<CrashPoint> {
        Some(match s {
            "authorized" => CrashPoint::Authorized,
            "logged" => CrashPoint::Logged,
            "intent" => CrashPoint::Intent,
            "sent" => CrashPoint::Sent,
            "confirmed" => CrashPoint::Confirmed,
            "closed" => CrashPoint::Closed,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            CrashPoint::Authorized => "authorized",
            CrashPoint::Logged => "logged",
            CrashPoint::Intent => "intent",
            CrashPoint::Sent => "sent",
            CrashPoint::Confirmed => "confirmed",
            CrashPoint::Closed => "closed",
        }
    }
}

/// A mensagem do pânico de uma queda simulada.
pub const CRASH: &str = "aletheia: queda simulada do processo";

/// Como a queda injetada acontece.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CrashMode {
    /// Pânico dentro do processo (os diferenciais do modelo: o mesmo processo observa o log).
    #[default]
    Panic,
    /// `SIGKILL` no próprio processo: encerramento forçado real, sem desempilhar, sem
    /// destrutores, sem descarregar buffers. Só o que já foi sincronizado no disco sobrevive.
    Kill,
}

/// Por que um WAL não pôde ser lido. **Nenhuma destas falhas vira um log vazio**: quem abre o
/// log recebe o erro e a recuperação não prossegue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalError {
    /// O arquivo existe e não pôde ser lido (permissão, E/S). `kind` é o `io::ErrorKind`.
    Io { path: String, kind: String, msg: String },
    /// Uma linha **inteira** (terminada por `\n`) que não é um registro válido, ou que quebra a
    /// cadeia de hashes: corrupção ou adulteração. `line` conta a partir de 1; `valid` é quantos
    /// registros íntegros a precedem.
    Corrupt { path: String, line: usize, valid: usize, reason: String },
    /// outro processo detém a trava do WAL: um executor por WAL
    Busy { path: String },
}

impl WalError {
    /// O código estável do diagnóstico (`athc recover` o imprime e o usa na saída).
    pub fn code(&self) -> &'static str {
        match self {
            WalError::Io { .. } => "W0001",
            WalError::Corrupt { .. } => "W0002",
            WalError::Busy { .. } => "W0006",
        }
    }
}

impl std::fmt::Display for WalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WalError::Io { path, kind, msg } => {
                write!(f, "{}: o WAL `{path}` existe e não pôde ser lido ({kind}): {msg}", self.code())
            }
            WalError::Corrupt { path, line, valid, reason } => write!(
                f,
                "{}: o WAL `{path}` está corrompido na linha {line} ({reason}); {valid} registro(s) íntegro(s) antes dela",
                self.code()
            ),
            WalError::Busy { path } => write!(
                f,
                "{}: outro processo está executando ou recuperando o WAL `{path}`; um executor por WAL",
                self.code()
            ),
        }
    }
}

/// O que a leitura encontrou além dos registros íntegros.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WalRead {
    /// o arquivo não existia: um log novo
    pub fresh: bool,
    /// a última linha estava interrompida (sem `\n`): a escrita que a queda cortou. Um registro
    /// está inteiro ou ausente; os bytes descartados ficam aqui e em `<wal>.torn`.
    pub torn: Option<Vec<u8>>,
}

#[derive(Debug, Default)]
pub struct Wal {
    pub log: Journal,
    path: Option<PathBuf>,
    /// a queda injetada: no ponto `.0` da `.1`-ésima operação com chave (a partir de 1)
    pub crash: Option<(CrashPoint, u32)>,
    pub crash_mode: CrashMode,
    pub nth: u32,
    /// a ordem de cada chave (a `n`-ésima operação com chave)
    pub ord: std::collections::HashMap<u64, u32>,
    /// o que a abertura encontrou
    pub read: WalRead,
    /// a trava exclusiva (`<wal>.lock`), mantida enquanto o `Wal` vive; o núcleo a solta se o
    /// processo morre
    lock: Option<std::fs::File>,
}

#[cfg(unix)]
fn kill_self() -> ! {
    extern "C" {
        fn getpid() -> i32;
        fn kill(pid: i32, sig: i32) -> i32;
    }
    // SAFETY: `getpid` e `kill` são chamadas POSIX sem pré-condições de memória.
    unsafe {
        kill(getpid(), 9);
    }
    // SIGKILL não pode ser tratado nem ignorado; se a entrega atrasar, não segue adiante.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

#[cfg(not(unix))]
fn kill_self() -> ! {
    std::process::abort()
}

fn io_err(path: &Path, e: &std::io::Error) -> WalError {
    WalError::Io { path: path.display().to_string(), kind: format!("{:?}", e.kind()), msg: e.to_string() }
}

impl Wal {
    /// Um log em arquivo: os registros de uma execução anterior são carregados e conferidos.
    ///
    /// * arquivo ausente: log novo (`read.fresh`);
    /// * erro de leitura (permissão, E/S): `WalError::Io` — **não** um log vazio;
    /// * linha inteira inválida, fora da cadeia ou adulterada: `WalError::Corrupt`;
    /// * última linha interrompida (sem `\n`): descartada, guardada em `<wal>.torn`, e o arquivo
    ///   é truncado no último registro íntegro, para que o próximo registro continue a cadeia.
    pub fn open(path: PathBuf) -> Result<Wal, WalError> {
        // a trava vem antes de qualquer leitura: reparar o rabo rasgado já é escrever
        let lp = PathBuf::from(format!("{}.lock", path.display()));
        let lf = std::fs::OpenOptions::new().create(true).append(true).open(&lp).map_err(|e| io_err(&path, &e))?;
        match crate::lock::exclusive(&lf, false) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                return Err(WalError::Busy { path: path.display().to_string() })
            }
            Err(e) => return Err(io_err(&path, &e)),
        }
        let lock = Some(lf);
        let bytes = match std::fs::read(&path) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(io_err(&path, &e)),
        };
        let Some(bytes) = bytes else {
            return Ok(Wal { path: Some(path), lock, read: WalRead { fresh: true, torn: None }, ..Wal::default() });
        };
        let (log, torn) = Wal::read(&bytes, &path.display().to_string())?;
        if let Some(t) = &torn {
            let side = PathBuf::from(format!("{}.torn", path.display()));
            let keep = (bytes.len() - t.len()) as u64;
            let fix = || -> std::io::Result<()> {
                let mut s = std::fs::OpenOptions::new().create(true).append(true).open(&side)?;
                s.write_all(t)?;
                s.write_all(b"\n")?;
                s.sync_all()?;
                let f = std::fs::OpenOptions::new().write(true).open(&path)?;
                f.set_len(keep)?;
                f.sync_all()
            };
            fix().map_err(|e| io_err(&path, &e))?;
        }
        Ok(Wal { log, path: Some(path), lock, read: WalRead { fresh: false, torn }, ..Wal::default() })
    }

    /// Um log só em memória (testes).
    pub fn memory() -> Wal {
        Wal::default()
    }

    /// O log que sobrou de uma execução anterior (o reinício depois da queda).
    pub fn resume(log: Journal) -> Wal {
        Wal { log, ..Wal::default() }
    }

    /// Lê os bytes de um log. Cada linha terminada por `\n` precisa ser um registro da cadeia;
    /// a primeira que não for é `Corrupt`. Só o resto **sem** `\n` no fim do arquivo é uma
    /// escrita interrompida: devolvido à parte, nunca interpretado.
    pub fn read(bytes: &[u8], path: &str) -> Result<(Journal, Option<Vec<u8>>), WalError> {
        let mut ok: Vec<Json> = Vec::new();
        let mut prev = GENESIS.to_string();
        let mut rest = bytes;
        let mut line_no = 0;
        while let Some(nl) = rest.iter().position(|&b| b == b'\n') {
            let (line, tail) = rest.split_at(nl);
            rest = &tail[1..];
            line_no += 1;
            let bad = |reason: &str| WalError::Corrupt {
                path: path.to_string(),
                line: line_no,
                valid: ok.len(),
                reason: reason.to_string(),
            };
            let text = std::str::from_utf8(line).map_err(|_| bad("não é UTF-8"))?;
            let e = serde_json::from_str::<Json>(text).map_err(|_| bad("não é JSON"))?;
            let mut body = e.clone();
            let obj = body.as_object_mut().ok_or_else(|| bad("não é um registro"))?;
            let h = obj.remove("hash").ok_or_else(|| bad("registro sem `hash`"))?;
            if obj.get("prev").and_then(|p| p.as_str()) != Some(prev.as_str()) {
                return Err(bad("`prev` não continua a cadeia"));
            }
            if h.as_str() != Some(entry_hash(&body).as_str()) {
                return Err(bad("o hash não confere com o conteúdo"));
            }
            prev = h.as_str().unwrap_or_default().to_string();
            ok.push(e);
        }
        let torn = if rest.is_empty() { None } else { Some(rest.to_vec()) };
        Ok((Journal::from_entries(ok), torn))
    }

    /// Acrescenta um registro e o sincroniza no disco antes de voltar. Se o log não pode ser
    /// escrito, o processo para aqui: nenhum efeito sai sem a intenção persistida.
    /// Este processo detém a trava exclusiva do WAL (falso só para um WAL em memória).
    pub fn locked(&self) -> bool {
        self.lock.is_some()
    }

    /// O hash do último registro: a identidade de uma intenção para o registro de consumo.
    pub fn last_hash(&self) -> Option<String> {
        self.log.entries.last().and_then(|e| e["hash"].as_str().map(String::from))
    }

    pub fn append(&mut self, t: f64, kind: &str, detail: Json) {
        self.log.push(t, kind, detail);
        if let Some(p) = &self.path {
            let line = self.log.entries.last().map(|e| e.to_string()).unwrap_or_default();
            let created = !p.exists();
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
                .unwrap_or_else(|e| panic!("wal: não foi possível abrir `{}` para escrita: {e}", p.display()));
            f.write_all(format!("{line}\n").as_bytes())
                .unwrap_or_else(|e| panic!("wal: não foi possível escrever em `{}`: {e}", p.display()));
            f.sync_all().unwrap_or_else(|e| panic!("wal: não foi possível sincronizar `{}`: {e}", p.display()));
            if created {
                // a entrada do diretório também precisa chegar ao disco
                if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
                    if let Ok(d) = std::fs::File::open(dir) {
                        let _ = d.sync_all();
                    }
                }
            }
        }
    }

    /// A queda injetada, se for este o ponto da operação de chave `key`.
    pub fn maybe_crash_key(&self, at: CrashPoint, key: u64) {
        if let (Some((p, n)), Some(&o)) = (self.crash, self.ord.get(&key)) {
            if p == at && n == o {
                match self.crash_mode {
                    CrashMode::Panic => panic!("{CRASH} ({} da operação {n})", at.name()),
                    CrashMode::Kill => kill_self(),
                }
            }
        }
    }

    pub fn to_jsonl(&self) -> String {
        self.log.to_jsonl()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> String {
        let mut w = Wal::memory();
        w.append(0.0, "intent", json!({ "key": "k1", "tool": "pay" }));
        w.append(1.0, "close", json!({ "key": "k1", "status": "done" }));
        w.append(2.0, "intent", json!({ "key": "k2", "tool": "pay" }));
        w.to_jsonl()
    }

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wal-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("w.jsonl")
    }

    /// Uma última linha interrompida (queda no meio da escrita) é descartada e devolvida à
    /// parte. Um registro está inteiro ou ausente.
    #[test]
    fn torn_tail_is_reported_and_discarded() {
        let text = sample();
        let torn = &text.as_bytes()[..text.len() - 17];
        let (back, rest) = Wal::read(torn, "t").unwrap();
        assert_eq!(back.entries.len(), 2);
        assert!(rest.is_some_and(|r| !r.is_empty()));
        assert_eq!(Journal::verify(&back.entries), Ok(()));
        let (all, rest) = Wal::read(text.as_bytes(), "t").unwrap();
        assert_eq!((all.entries.len(), rest), (3, None));
    }

    /// Corrupção **no meio** não é uma cauda rasgada: é erro, com a linha e os íntegros antes
    /// dela. A Rev13 cortava o log ali em silêncio e perdia as obrigações seguintes.
    #[test]
    fn interior_corruption_is_an_error_not_a_shorter_log() {
        let text = sample();
        let tampered = text.replacen("\"done\"", "\"failed\"", 1);
        let e = Wal::read(tampered.as_bytes(), "t").unwrap_err();
        assert!(matches!(&e, WalError::Corrupt { line: 2, valid: 1, .. }), "{e:?}");
        // um registro removido do meio quebra o `prev` do seguinte
        let lines: Vec<&str> = text.lines().collect();
        let gap = format!("{}\n{}\n", lines[0], lines[2]);
        assert!(matches!(Wal::read(gap.as_bytes(), "t"), Err(WalError::Corrupt { line: 2, valid: 1, .. })));
        // lixo binário numa linha inteira
        let mut junk = lines[0].as_bytes().to_vec();
        junk.extend_from_slice(b"\n\xff\xfe\x00garbage\n");
        junk.extend_from_slice(lines[1].as_bytes());
        junk.push(b'\n');
        assert!(matches!(Wal::read(&junk, "t"), Err(WalError::Corrupt { line: 2, valid: 1, .. })));
        // uma última linha INTEIRA (com `\n`) inválida também é corrupção, não cauda rasgada
        let bad_last = format!("{}\n{{\"x\":1}}\n", lines[0]);
        assert!(matches!(Wal::read(bad_last.as_bytes(), "t"), Err(WalError::Corrupt { line: 2, .. })));
    }

    #[test]
    fn missing_file_is_a_fresh_log() {
        let p = tmp("fresh");
        let w = Wal::open(p).unwrap();
        assert!(w.read.fresh && w.log.entries.is_empty());
    }

    /// O defeito da Rev13: `read_to_string(..).unwrap_or_default()` transformava qualquer falha
    /// de leitura num log vazio, isto é, em "nenhuma obrigação em aberto".
    #[test]
    fn unreadable_file_is_an_error_not_an_empty_log() {
        // um diretório no lugar do arquivo: a leitura falha em qualquer usuário (inclusive root)
        let p = tmp("dir");
        std::fs::create_dir_all(&p).unwrap();
        let e = Wal::open(p).unwrap_err();
        assert!(matches!(e, WalError::Io { .. }), "{e:?}");
        assert_eq!(e.code(), "W0001");
    }

    #[cfg(unix)]
    #[test]
    fn permission_denied_is_an_error_not_an_empty_log() {
        use std::os::unix::fs::PermissionsExt;
        let p = tmp("perm");
        std::fs::write(&p, sample()).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).unwrap();
        // root lê qualquer arquivo: o teste só vale se a leitura de fato for negada
        if std::fs::read(&p).is_err() {
            let e = Wal::open(p.clone()).unwrap_err();
            assert!(matches!(&e, WalError::Io { kind, .. } if kind == "PermissionDenied"), "{e:?}");
        }
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn file_wal_survives_reopen() {
        let p = tmp("reopen");
        let mut w = Wal::open(p.clone()).unwrap();
        w.append(0.0, "intent", json!({ "key": "k1" }));
        drop(w);
        let mut w = Wal::open(p.clone()).unwrap();
        assert_eq!(w.log.entries.len(), 1);
        assert!(!w.read.fresh);
        w.append(1.0, "escalate", json!({ "key": "k1" }));
        // um executor por WAL: enquanto `w` vive, uma segunda abertura é recusada (W0006)
        let busy = Wal::open(p.clone()).unwrap_err();
        assert_eq!(busy.code(), "W0006");
        drop(w);
        let st = WalState::of(&Wal::open(p).unwrap().log);
        assert_eq!(st.outcome.get("k1").map(String::as_str), Some("escalated"));
        assert!(st.open().is_empty());
    }

    /// Reabrir um arquivo com a cauda rasgada: a intenção inteira continua lá, o resto vai para
    /// `<wal>.torn`, o arquivo é truncado e o registro seguinte continua a cadeia.
    #[test]
    fn torn_file_is_truncated_and_the_chain_continues() {
        let p = tmp("torn");
        let text = sample();
        std::fs::write(&p, &text.as_bytes()[..text.len() - 9]).unwrap();
        let mut w = Wal::open(p.clone()).unwrap();
        assert_eq!(w.log.entries.len(), 2);
        assert!(w.read.torn.is_some());
        assert!(PathBuf::from(format!("{}.torn", p.display())).exists());
        w.append(3.0, "escalate", json!({ "key": "k9" }));
        drop(w);
        let again = Wal::open(p).unwrap();
        assert_eq!(again.log.entries.len(), 3);
        assert_eq!(again.read.torn, None);
        assert_eq!(Journal::verify(&again.log.entries), Ok(()));
    }

    #[test]
    fn corrupt_file_refuses_to_open() {
        let p = tmp("corrupt");
        std::fs::write(&p, sample().replacen("\"pay\"", "\"pwn\"", 1)).unwrap();
        let e = Wal::open(p).unwrap_err();
        assert_eq!(e.code(), "W0002");
        assert!(e.to_string().contains("linha 1"), "{e}");
    }
}

/// O estado de um log: as intenções, os desfechos e as sagas.
#[derive(Debug, Default)]
pub struct WalState {
    /// (chave, detalhe da intenção), em ordem
    pub intents: Vec<(String, Json)>,
    /// chave → desfecho (`done`, `failed`, `escalated`)
    pub outcome: std::collections::BTreeMap<String, String>,
    /// saga → (terminou, operações registradas em ordem, chaves tiradas do registro, compensações feitas)
    pub sagas: std::collections::BTreeMap<u64, SagaRec>,
    /// chave → hash do registro `intent` (a identidade que o registro de consumo cita)
    pub hashes: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Default, Clone)]
pub struct SagaRec {
    pub ended: bool,
    pub ops: Vec<Json>,
    pub unlogged: Vec<String>,
    pub compensated: usize,
}

impl WalState {
    pub fn of(log: &Journal) -> WalState {
        let mut s = WalState::default();
        for e in &log.entries {
            let d = &e["detail"];
            let key = || d["key"].as_str().unwrap_or_default().to_string();
            let saga = || d["saga"].as_u64().unwrap_or_default();
            match e["kind"].as_str() {
                Some("intent") => {
                    s.intents.push((key(), d.clone()));
                    s.hashes.insert(key(), e["hash"].as_str().unwrap_or_default().to_string());
                    // uma operação com chave dentro de saga pertence a ela desde a intenção: a
                    // queda pode vir antes de o quadro registrá-la (`saga_op`)
                    if let Some(sid) = d["saga"].as_u64() {
                        s.sagas.entry(sid).or_default().ops.push(d.clone());
                    }
                }
                Some("close") => {
                    s.outcome.insert(key(), d["status"].as_str().unwrap_or_default().to_string());
                }
                Some("escalate") => {
                    s.outcome.insert(key(), "escalated".into());
                }
                Some("saga_begin") => {
                    s.sagas.entry(saga()).or_default();
                }
                Some("saga_op") => {
                    let sg = s.sagas.entry(saga()).or_default();
                    if d["key"].is_null() || !sg.ops.iter().any(|o| o["key"] == d["key"]) {
                        sg.ops.push(d.clone());
                    }
                }
                Some("saga_unlog") => s.sagas.entry(saga()).or_default().unlogged.push(key()),
                Some("compensated") => s.sagas.entry(saga()).or_default().compensated += 1,
                Some("saga_end") => s.sagas.entry(saga()).or_default().ended = true,
                _ => {}
            }
        }
        s
    }

    /// As intenções sem desfecho.
    pub fn open(&self) -> Vec<&(String, Json)> {
        self.intents.iter().filter(|(k, _)| !self.outcome.contains_key(k)).collect()
    }
}

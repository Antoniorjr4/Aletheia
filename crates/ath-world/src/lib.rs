//! `ath-world`: um serviço externo **local e persistente** para a demonstração v1.0-RC1.
//!
//! É o mundo de fora do interpretador: um processo separado, com o estado num banco SQLite
//! (`synchronous = FULL`), que o ARX alcança por TCP (`arx::remote::RemoteHost`). Uma operação
//! confirmada aqui está no disco e é observável por qualquer outro processo
//! (`ath-world state --db …` lê o banco diretamente, sem passar pelo serviço).
//!
//! É uma tesouraria mínima, com as três classes de provedor que a linguagem distingue:
//!
//! | ferramenta | classe | o que o serviço oferece |
//! |---|---|---|
//! | `pay`      | `idempotent` | deduplica pela chave: reenviar devolve o mesmo recibo, sem novo efeito |
//! | `refund`   | `idempotent` | o estorno de um `pay`, pela mesma chave; total (sem o que estornar, nada faz) |
//! | `receipt`  | `idempotent` | anota um comprovante no razão; deduplica pela chave |
//! | `schedule` | `queryable`  | **não** deduplica (reenviar agendaria de novo), mas responde `status` pela chave |
//! | `wire`     | `opaque`     | não guarda a chave e não tem `status`: um resultado desconhecido não tem como ser resolvido |
//! | `balance`, `statement`, `conserved` | leitura | sem efeito; o texto de `statement` inclui o memorando de terceiros |
//!
//! Falhas controladas (tabela `faults`, por ferramenta e número da chamada): `refuse` (erro, sem
//! efeito), `lost` (fecha sem aplicar), `drop_reply` (aplica, confirma no disco e fecha sem
//! responder), `hang` (aplica, confirma e demora a responder: a janela para matar o cliente) e
//! `status_unknown` (a consulta de status responde "desconhecido").
//!
//! Protocolo: uma requisição JSON por conexão, uma linha; a resposta é uma linha JSON.

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value as Json};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};

pub const TREASURY: &str = "TREASURY";

/// Um decimal não negativo em milionésimos (a mesma leitura de `arx::host::decimal`).
pub fn decimal(x: &str) -> Option<i64> {
    let t: String = x.trim().chars().filter(|&ch| ch != ',').collect();
    let (int, frac) = t.split_once('.').unwrap_or((&t, ""));
    if int.is_empty() || int.len() > 12 || frac.len() > 6 {
        return None;
    }
    if !int.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let v: i64 = int.parse().ok()?;
    let f: i64 = format!("{frac:0<6}").parse().ok()?;
    v.checked_mul(1_000_000)?.checked_add(f)
}

pub fn show(micro: i64) -> String {
    format!("{}.{:02}", micro / 1_000_000, (micro % 1_000_000) / 10_000)
}

pub fn open(db: &Path) -> rusqlite::Result<Connection> {
    let c = Connection::open(db)?;
    c.pragma_update(None, "journal_mode", "WAL")?;
    c.pragma_update(None, "synchronous", "FULL")?;
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS accounts (id TEXT PRIMARY KEY, balance INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS ledger (
           seq INTEGER PRIMARY KEY AUTOINCREMENT, tool TEXT NOT NULL, key TEXT,
           account TEXT NOT NULL, amount INTEGER NOT NULL, memo TEXT NOT NULL, state TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS receipts (tool TEXT NOT NULL, key TEXT NOT NULL, result TEXT NOT NULL,
           PRIMARY KEY (tool, key));
         CREATE TABLE IF NOT EXISTS calls (tool TEXT PRIMARY KEY, n INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS faults (
           id INTEGER PRIMARY KEY AUTOINCREMENT, tool TEXT NOT NULL, nth INTEGER, kind TEXT NOT NULL,
           ms INTEGER NOT NULL DEFAULT 0, used INTEGER NOT NULL DEFAULT 0);
         CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS log (seq INTEGER PRIMARY KEY AUTOINCREMENT, line TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS anchors (scope TEXT PRIMARY KEY, seq INTEGER NOT NULL, head TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS lc_scopes (scope TEXT PRIMARY KEY, meta TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS lc_entries (scope TEXT NOT NULL, seq INTEGER NOT NULL, entry TEXT NOT NULL,
                                                PRIMARY KEY (scope, seq));
         CREATE TABLE IF NOT EXISTS clock_last (id INTEGER PRIMARY KEY CHECK (id = 1), t INTEGER NOT NULL, resp TEXT NOT NULL);",
    )?;
    Ok(c)
}

/// Cria as contas de `{"accounts": {"ID": "saldo decimal", …}}` (substitui o estado).
pub fn seed(c: &Connection, j: &Json) -> Result<(), String> {
    let accounts = j.get("accounts").and_then(|a| a.as_object()).ok_or("semente sem `accounts`")?;
    let e = |x: rusqlite::Error| x.to_string();
    c.execute_batch("DELETE FROM accounts; DELETE FROM ledger; DELETE FROM receipts; DELETE FROM calls; DELETE FROM faults; DELETE FROM log; DELETE FROM meta;")
        .map_err(e)?;
    for (id, bal) in accounts {
        let v = bal.as_str().and_then(decimal).ok_or_else(|| format!("saldo inválido para `{id}`"))?;
        c.execute("INSERT INTO accounts (id, balance) VALUES (?1, ?2)", params![id, v]).map_err(e)?;
    }
    // o invariante da partida dobrada: a soma dos saldos não muda
    c.execute("INSERT INTO meta (k, v) SELECT 'total', COALESCE(SUM(balance), 0) FROM accounts", []).map_err(e)?;
    // lançamentos anteriores de terceiros: o memorando é texto de fora (`statement` o devolve)
    for m in j.get("memos").and_then(|m| m.as_array()).into_iter().flatten() {
        let memo = m.as_str().ok_or("memorando não textual")?;
        c.execute(
            "INSERT INTO ledger (tool, key, account, amount, memo, state) VALUES ('incoming', NULL, ?1, 0, ?2, 'note')",
            params![TREASURY, memo],
        )
        .map_err(e)?;
    }
    Ok(())
}

pub fn add_fault(c: &Connection, tool: &str, nth: Option<i64>, kind: &str, ms: i64) -> Result<(), String> {
    if !["refuse", "lost", "drop_reply", "hang", "status_unknown", "skew", "freeze", "replay"].contains(&kind) {
        return Err(format!("falha desconhecida `{kind}`"));
    }
    c.execute("INSERT INTO faults (tool, nth, kind, ms) VALUES (?1, ?2, ?3, ?4)", params![tool, nth, kind, ms])
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// O estado observável: saldos, razão e contagens. É o que `ath-world state` imprime.
pub fn state(c: &Connection) -> rusqlite::Result<Json> {
    let mut accounts = serde_json::Map::new();
    let mut st = c.prepare("SELECT id, balance FROM accounts ORDER BY id")?;
    for row in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
        let (id, b) = row?;
        accounts.insert(id, json!(show(b)));
    }
    let mut ledger = Vec::new();
    let mut st = c.prepare("SELECT seq, tool, key, account, amount, memo, state FROM ledger ORDER BY seq")?;
    let rows = st.query_map([], |r| {
        Ok(json!({ "seq": r.get::<_, i64>(0)?, "tool": r.get::<_, String>(1)?, "key": r.get::<_, Option<String>>(2)?,
                   "account": r.get::<_, String>(3)?, "amount": show(r.get::<_, i64>(4)?), "memo": r.get::<_, String>(5)?,
                   "state": r.get::<_, String>(6)? }))
    })?;
    for row in rows {
        ledger.push(row?);
    }
    let count = |tool: &str, state: &str| ledger.iter().filter(|e| e["tool"] == tool && e["state"] == state).count();
    // a marca da âncora (v1.0-RC3): quantos consumos estão ancorados, no escopo mais adiantado
    let anchored: i64 = c.query_row("SELECT COALESCE(MAX(seq), 0) FROM anchors", [], |r| r.get(0))?;
    // o registro unificado (v1.0-RC4): quantos registros de cada tipo
    let mut lc = serde_json::Map::new();
    let mut st = c.prepare("SELECT entry FROM lc_entries")?;
    for e in st.query_map([], |r| r.get::<_, String>(0))? {
        let e: Json = serde_json::from_str(&e?).unwrap_or_default();
        let k = e["kind"].as_str().unwrap_or("?").to_string();
        let n = lc.get(&k).and_then(|v| v.as_u64()).unwrap_or(0) + 1;
        lc.insert(k, json!(n));
    }
    Ok(json!({
        "anchored": anchored,
        "lifecycle": lc,
        "accounts": accounts,
        "counts": { "pays_posted": count("pay", "posted"), "pays_refunded": count("pay", "reversed"),
                    "scheduled": count("schedule", "scheduled"), "wires": count("wire", "sent"),
                    "receipts": count("receipt", "noted") },
        "ledger": ledger,
    }))
}

/// A falha a injetar nesta chamada (a `n`-ésima da ferramenta), consumida se for de uso único.
fn fault(c: &Connection, tool: &str, n: i64, kinds: &[&str]) -> rusqlite::Result<Option<(String, i64)>> {
    let mut st = c.prepare("SELECT id, kind, ms, nth FROM faults WHERE tool = ?1 AND used = 0 ORDER BY id")?;
    let rows: Vec<(i64, String, i64, Option<i64>)> =
        st.query_map(params![tool], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?;
    for (id, kind, ms, nth) in rows {
        if kinds.contains(&kind.as_str()) && nth.is_none_or(|k| k == n) {
            c.execute("UPDATE faults SET used = 1 WHERE id = ?1", params![id])?;
            return Ok(Some((kind, ms)));
        }
    }
    Ok(None)
}

fn text(args: &[Json], i: usize) -> Result<String, String> {
    match args.get(i) {
        Some(Json::String(s)) => Ok(s.clone()),
        Some(Json::Number(n)) => Ok(n.to_string()),
        _ => Err(format!("argumento {} ausente ou não textual", i + 1)),
    }
}

fn amount(args: &[Json], i: usize) -> Result<i64, String> {
    decimal(&text(args, i)?).filter(|v| *v > 0).ok_or_else(|| "valor inválido".to_string())
}

/// Aplica a operação dentro da transação `c`. `Err` é uma recusa do provedor: nada mudou.
fn apply(c: &Connection, tool: &str, key: Option<&str>, args: &[Json]) -> Result<Json, String> {
    let e = |x: rusqlite::Error| x.to_string();
    let balance = |id: &str| -> Result<i64, String> {
        c.query_row("SELECT balance FROM accounts WHERE id = ?1", params![id], |r| r.get(0))
            .optional()
            .map_err(e)?
            .ok_or_else(|| format!("conta `{id}` não existe"))
    };
    let mv = |from: &str, to: &str, v: i64| -> Result<(), String> {
        if balance(from)? < v {
            return Err(format!("saldo insuficiente em `{from}`"));
        }
        balance(to)?;
        c.execute("UPDATE accounts SET balance = balance - ?2 WHERE id = ?1", params![from, v]).map_err(e)?;
        c.execute("UPDATE accounts SET balance = balance + ?2 WHERE id = ?1", params![to, v]).map_err(e)?;
        Ok(())
    };
    let post = |state: &str, to: &str, v: i64, memo: &str, key: Option<&str>| -> Result<i64, String> {
        c.execute(
            "INSERT INTO ledger (tool, key, account, amount, memo, state) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![tool, key, to, v, memo, state],
        )
        .map_err(e)?;
        Ok(c.last_insert_rowid())
    };
    match tool {
        "balance" => Ok(json!(show(balance(&text(args, 0)?)?))),
        "statement" => {
            let mut st = c
                .prepare("SELECT seq, tool, account, amount, memo, state FROM ledger ORDER BY seq DESC LIMIT 20")
                .map_err(e)?;
            let lines: Vec<String> = st
                .query_map([], |r| {
                    Ok(format!(
                        "#{} {} {} {} [{}] memo: {}",
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        show(r.get::<_, i64>(3)?),
                        r.get::<_, String>(5)?,
                        r.get::<_, String>(4)?
                    ))
                })
                .map_err(e)?
                .collect::<Result<_, _>>()
                .map_err(e)?;
            Ok(json!(lines.join("\n")))
        }
        // idempotent: a chave deduplica (quem chama `apply` já consultou os recibos)
        "conserved" => {
            let (sum, total): (i64, i64) = c
                .query_row(
                    "SELECT (SELECT COALESCE(SUM(balance), 0) FROM accounts), (SELECT v FROM meta WHERE k = 'total')",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(e)?;
            Ok(json!(sum == total))
        }
        "pay" => {
            let (to, v, memo) = (text(args, 0)?, amount(args, 1)?, text(args, 2).unwrap_or_default());
            mv(TREASURY, &to, v)?;
            let seq = post("posted", &to, v, &memo, key)?;
            Ok(json!(format!("pay #{seq} {} -> {to}", show(v))))
        }
        "receipt" => {
            let note = text(args, 0)?;
            let seq = post("noted", TREASURY, 0, &note, key)?;
            Ok(json!(format!("receipt #{seq}")))
        }
        // o estorno de um `pay`, pela chave dele; total: sem lançamento, nada a estornar
        "refund" => {
            let Some(k) = key else { return Err("`refund` exige a chave da operação".into()) };
            let row: Option<(i64, String, i64)> = c
                .query_row(
                    "SELECT seq, account, amount FROM ledger WHERE tool = 'pay' AND key = ?1 AND state = 'posted'",
                    params![k],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(e)?;
            match row {
                Some((seq, to, v)) => {
                    mv(&to, TREASURY, v)?;
                    c.execute("UPDATE ledger SET state = 'reversed' WHERE seq = ?1", params![seq]).map_err(e)?;
                    Ok(json!(format!("refund #{seq} {} <- {to}", show(v))))
                }
                None => Ok(json!("refund: nada a estornar")),
            }
        }
        // queryable: não deduplica; a chave fica no lançamento para a consulta de status
        "schedule" => {
            let (to, v, date) = (text(args, 0)?, amount(args, 1)?, text(args, 2).unwrap_or_default());
            balance(&to)?;
            let seq = post("scheduled", &to, v, &date, key)?;
            Ok(json!(format!("schedule #{seq} {} -> {to} em {date}", show(v))))
        }
        // opaque: o efeito acontece e a chave não é guardada em lugar nenhum
        "wire" => {
            let (to, v) = (text(args, 0)?, amount(args, 1)?);
            mv(TREASURY, &to, v)?;
            let seq = post("sent", &to, v, "", None)?;
            Ok(json!(format!("wire #{seq} {} -> {to}", show(v))))
        }
        other => Err(format!("ferramenta `{other}` desconhecida")),
    }
}

enum Reply {
    Line(Json),
    /// fecha a conexão sem responder
    Silence,
    /// responde depois de `ms`
    Late(Json, u64),
}

fn handle(db: &Mutex<Connection>, req: &Json) -> Reply {
    let mut c = db.lock().unwrap_or_else(|p| p.into_inner());
    let err = |m: String| Reply::Line(json!({ "ok": false, "error": m }));
    let tool = req["tool"].as_str().unwrap_or_default().to_string();
    let key = req["key"].as_str().map(String::from);
    let log = |c: &Connection, line: String| {
        let _ = c.execute("INSERT INTO log (line) VALUES (?1)", params![line]);
    };
    match req["op"].as_str() {
        Some("ping") => Reply::Line(json!({ "ok": true })),
        // **A âncora** (v1.0-RC3, `arx::anchor`): por escopo, a marca do registro de consumo.
        // `advance` é um compare-and-swap que só anda um passo para a frente; nunca volta. O
        // serviço não vê concessões, só hashes. Num deployment, é um processo e um banco
        // separados dos do provedor (`ath-world serve` em outra porta, outro arquivo).
        Some("anchor") => {
            let Some(scope) = req["scope"].as_str() else { return err("âncora sem escopo".into()) };
            let cur: rusqlite::Result<Option<(i64, String)>> = c
                .query_row("SELECT seq, head FROM anchors WHERE scope = ?1", params![scope], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .optional();
            let (seq, head) = match cur {
                Ok(Some(x)) => x,
                Ok(None) => (0, "0".repeat(64)),
                Err(e) => return err(e.to_string()),
            };
            if req["do"] == "read" {
                return Reply::Line(json!({ "ok": true, "seq": seq, "head": head }));
            }
            let m = |j: &Json| Some((j["seq"].as_i64()?, j["head"].as_str()?.to_string()));
            let (Some(from), Some(to)) = (m(&req["from"]), m(&req["to"])) else {
                return err("avanço sem marcas".into());
            };
            if (seq, head.clone()) == to {
                return Reply::Line(json!({ "ok": true, "seq": seq, "head": head }));
            }
            if (seq, head.clone()) != from || to.0 != from.0 + 1 {
                log(
                    &c,
                    format!(
                        "anchor {} recusado: atual {seq}, pedido {} -> {}",
                        &scope[..12.min(scope.len())],
                        from.0,
                        to.0
                    ),
                );
                return Reply::Line(json!({ "ok": false, "conflict": true, "seq": seq, "head": head }));
            }
            match c.execute(
                "INSERT INTO anchors (scope, seq, head) VALUES (?1, ?2, ?3)
                 ON CONFLICT(scope) DO UPDATE SET seq = excluded.seq, head = excluded.head",
                params![scope, to.0, to.1],
            ) {
                Ok(_) => Reply::Line(json!({ "ok": true, "seq": to.0, "head": to.1 })),
                Err(e) => err(e.to_string()),
            }
        }
        Some("state") => match state(&c) {
            Ok(s) => Reply::Line(json!({ "ok": true, "value": s })),
            Err(e) => err(e.to_string()),
        },
        Some("status") => {
            let Some(k) = key else { return err("status sem chave".into()) };
            if matches!(fault(&c, &tool, 0, &["status_unknown"]), Ok(Some(_))) {
                log(&c, format!("status {tool} {k}: unknown (falha injetada)"));
                return Reply::Line(json!({ "ok": true, "status": "unknown" }));
            }
            let r: rusqlite::Result<Option<String>> = match tool.as_str() {
                "pay" | "refund" | "receipt" => c
                    .query_row("SELECT result FROM receipts WHERE tool = ?1 AND key = ?2", params![tool, k], |r| r.get(0))
                    .optional(),
                "schedule" => c
                    .query_row(
                        "SELECT 'schedule #' || seq FROM ledger WHERE tool = 'schedule' AND key = ?1 ORDER BY seq LIMIT 1",
                        params![k],
                        |r| r.get(0),
                    )
                    .optional(),
                // opaque: o serviço não tem como saber
                _ => return Reply::Line(json!({ "ok": true, "status": "unknown" })),
            };
            // toda consulta fica no registro do serviço (v1.0-RC4: a recuperação que volta ao
            // provedor por um desfecho já confirmado tem de ser observável)
            match r {
                Ok(Some(v)) => {
                    log(&c, format!("status {tool} {k}: done"));
                    Reply::Line(json!({ "ok": true, "status": "done", "value": v }))
                }
                Ok(None) => {
                    log(&c, format!("status {tool} {k}: not_applied"));
                    Reply::Line(json!({ "ok": true, "status": "not_applied" }))
                }
                Err(e) => err(e.to_string()),
            }
        }
        Some("call") => {
            let args: Vec<Json> = req["args"].as_array().cloned().unwrap_or_default();
            let idempotent = matches!(tool.as_str(), "pay" | "refund" | "receipt");
            // idempotent: a chave já vista devolve o recibo, sem contar a chamada nem aplicar
            if let (true, Some(k)) = (idempotent, &key) {
                let prev: rusqlite::Result<Option<String>> = c
                    .query_row("SELECT result FROM receipts WHERE tool = ?1 AND key = ?2", params![tool, k], |r| {
                        r.get(0)
                    })
                    .optional();
                match prev {
                    Ok(Some(r)) => {
                        log(&c, format!("call {tool} {k}: recibo repetido (sem novo efeito)"));
                        return Reply::Line(json!({ "ok": true, "value": r, "replayed": true }));
                    }
                    Ok(None) => {}
                    Err(e) => return err(e.to_string()),
                }
            }
            let tx = match c.transaction() {
                Ok(t) => t,
                Err(e) => return err(e.to_string()),
            };
            let n: i64 = tx
                .query_row(
                    "INSERT INTO calls (tool, n) VALUES (?1, 1) ON CONFLICT(tool) DO UPDATE SET n = n + 1 RETURNING n",
                    params![tool],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            let f = fault(&tx, &tool, n, &["refuse", "lost", "drop_reply", "hang"]).ok().flatten();
            let shown = key.clone().unwrap_or_else(|| "-".into());
            match f.as_ref().map(|(k, _)| k.as_str()) {
                Some("refuse") => {
                    log(&tx, format!("call {tool} {shown}: recusada (falha injetada)"));
                    let _ = tx.commit();
                    return err(format!("`{tool}` recusada pelo provedor"));
                }
                Some("lost") => {
                    log(&tx, format!("call {tool} {shown}: perdida antes de aplicar (falha injetada)"));
                    let _ = tx.commit();
                    return Reply::Silence;
                }
                _ => {}
            }
            let out = match apply(&tx, &tool, key.as_deref(), &args) {
                Ok(v) => v,
                Err(m) => {
                    // recusa do provedor: a transação é desfeita; só a contagem e o registro ficam
                    drop(tx);
                    let _ = c.execute(
                        "INSERT INTO calls (tool, n) VALUES (?1, 1) ON CONFLICT(tool) DO UPDATE SET n = n + 1",
                        params![tool],
                    );
                    log(&c, format!("call {tool} {shown}: recusada: {m}"));
                    return err(m);
                }
            };
            if let (true, Some(k)) = (idempotent, &key) {
                let r = out.as_str().unwrap_or_default();
                if let Err(e) =
                    tx.execute("INSERT INTO receipts (tool, key, result) VALUES (?1, ?2, ?3)", params![tool, k, r])
                {
                    return err(e.to_string());
                }
            }
            let note = match f.as_ref().map(|(k, _)| k.as_str()) {
                Some("drop_reply") => " (aplicada; resposta descartada)",
                Some("hang") => " (aplicada; resposta atrasada)",
                _ => "",
            };
            log(&tx, format!("call {tool} {shown}: aplicada{note}"));
            // a resposta só sai depois de o efeito estar no disco
            if let Err(e) = tx.commit() {
                return err(e.to_string());
            }
            let reply = json!({ "ok": true, "value": out });
            match f {
                Some((k, _)) if k == "drop_reply" => Reply::Silence,
                Some((k, ms)) if k == "hang" => Reply::Late(reply, ms.max(0) as u64),
                _ => Reply::Line(reply),
            }
        }
        _ => err("operação desconhecida".into()),
    }
}

fn serve_one(svc: &Svc, stream: TcpStream) {
    let mut line = String::new();
    let Ok(clone) = stream.try_clone() else { return };
    if BufReader::new(clone).read_line(&mut line).is_err() {
        return;
    }
    let Ok(req) = serde_json::from_str::<Json>(&line) else { return };
    let mut stream = stream;
    match dispatch(svc, &req) {
        Reply::Silence => {}
        Reply::Line(j) => {
            let _ = stream.write_all(format!("{j}\n").as_bytes());
        }
        Reply::Late(j, ms) => {
            // o banco já foi liberado: outros processos observam o efeito durante a espera
            std::thread::sleep(std::time::Duration::from_millis(ms));
            let _ = stream.write_all(format!("{j}\n").as_bytes());
        }
    }
}

/// Atende até o processo ser encerrado. Uma thread por conexão; o banco é serializado.
pub fn serve(listener: TcpListener, conn: Connection) {
    serve_with(listener, conn, None)
}

/// O serviço com uma chave de assinatura (v1.0-RC4): com ela, ele responde como **testemunha
/// de tempo** (operação `time`) e devolve **recibos assinados** dos desfechos (`call`, `status`).
/// Sem ela, como antes.
pub fn serve_with(listener: TcpListener, conn: Connection, key: Option<[u8; 32]>) {
    let svc = Arc::new(Svc { db: Mutex::new(conn), key });
    for stream in listener.incoming().flatten() {
        let svc = svc.clone();
        std::thread::spawn(move || serve_one(&svc, stream));
    }
}

pub struct Svc {
    db: Mutex<Connection>,
    key: Option<[u8; 32]>,
}

fn now_ceil() -> u64 {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    d.as_secs() + u64::from(d.subsec_nanos() > 0)
}

/// **A testemunha de tempo.** Assina o instante (arredondado para cima) sobre o nonce do pedido.
/// Falhas injetáveis (ferramenta `time`), para os testes do verificador do executor: `skew`
/// (desloca `ms` segundos, com sinal), `freeze` (repete o último instante, assinado de novo),
/// `replay` (devolve a última resposta inteira, de outro pedido) e `hang` (atrasa `ms`).
fn time_reply(svc: &Svc, req: &Json) -> Reply {
    let err = |m: &str| Reply::Line(json!({ "ok": false, "error": m }));
    let Some(key) = svc.key else { return err("este serviço não é testemunha de tempo (sem --sign-key)") };
    let Some(nonce) = req["nonce"].as_str() else { return err("pedido de tempo sem nonce") };
    let c = svc.db.lock().unwrap_or_else(|p| p.into_inner());
    let n: i64 = c
        .query_row(
            "INSERT INTO calls (tool, n) VALUES ('time', 1) ON CONFLICT(tool) DO UPDATE SET n = n + 1 RETURNING n",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let last: Option<(i64, String)> = c
        .query_row("SELECT t, resp FROM clock_last WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()
        .ok()
        .flatten();
    let f = fault(&c, "time", n, &["skew", "freeze", "replay", "hang"]).ok().flatten();
    let mut t = now_ceil() as i64;
    match f.as_ref().map(|(k, ms)| (k.as_str(), *ms)) {
        Some(("replay", _)) => {
            if let Some((_, resp)) = last {
                return Reply::Line(serde_json::from_str(&resp).unwrap_or_default());
            }
        }
        Some(("freeze", _)) => t = last.map_or(t, |l| l.0),
        Some(("skew", s)) => t += s,
        _ => {}
    }
    let t = t.max(0) as u64;
    let sig = arx::sign::sign(&key, "time", &arx::lifecycle::time_payload(nonce, t));
    let reply = json!({ "ok": true, "t": t, "nonce": nonce, "sig": sig });
    let _ = c.execute(
        "INSERT INTO clock_last (id, t, resp) VALUES (1, ?1, ?2) ON CONFLICT(id) DO UPDATE SET t = ?1, resp = ?2",
        params![t as i64, reply.to_string()],
    );
    match f {
        Some((k, ms)) if k == "hang" => Reply::Late(reply, ms.max(0) as u64),
        _ => Reply::Line(reply),
    }
}

/// **O registro unificado** (v1.0-RC4, `arx::lifecycle`). Por escopo, os registros do ciclo de
/// vida, cada um aceito pelas regras de `arx::lifecycle::accept`: evidência, não forma. O escopo
/// é aberto com a sua configuração de confiança, e o identificador dele é o hash dela.
fn lifecycle_reply(svc: &Svc, req: &Json) -> Reply {
    let err = |m: String| Reply::Line(json!({ "ok": false, "error": m }));
    let Some(scope) = req["scope"].as_str() else { return err("registro sem escopo".into()) };
    let mut c = svc.db.lock().unwrap_or_else(|p| p.into_inner());
    let meta_of = |c: &Connection| -> Option<arx::lifecycle::Meta> {
        let m: String =
            c.query_row("SELECT meta FROM lc_scopes WHERE scope = ?1", params![scope], |r| r.get(0)).ok()?;
        arx::lifecycle::Meta::from_json(&serde_json::from_str(&m).ok()?)
    };
    let chain_of = |c: &Connection| -> Vec<arx::lifecycle::Entry> {
        let Ok(mut st) = c.prepare("SELECT entry FROM lc_entries WHERE scope = ?1 ORDER BY seq") else {
            return Vec::new();
        };
        st.query_map(params![scope], |r| r.get::<_, String>(0))
            .map(|rows| {
                rows.flatten()
                    .filter_map(|e| serde_json::from_str(&e).ok().and_then(|j| arx::lifecycle::Entry::from_json(&j)))
                    .collect()
            })
            .unwrap_or_default()
    };
    match req["do"].as_str() {
        Some("open") => {
            let Some(meta) = arx::lifecycle::Meta::from_json(&req["meta"]) else {
                return err("configuração ilegível".into());
            };
            if meta.scope() != scope {
                return err("o escopo não é o hash da configuração de confiança".into());
            }
            match c.execute(
                "INSERT OR IGNORE INTO lc_scopes (scope, meta) VALUES (?1, ?2)",
                params![scope, meta.to_json().to_string()],
            ) {
                Ok(_) => Reply::Line(json!({ "ok": true })),
                Err(e) => err(e.to_string()),
            }
        }
        Some("read") => {
            let chain: Vec<Json> = chain_of(&c).iter().map(|e| e.to_json()).collect();
            Reply::Line(json!({ "ok": true, "chain": chain }))
        }
        Some("append") => {
            let Some(meta) = meta_of(&c) else { return err("escopo não aberto".into()) };
            let mut chain = chain_of(&c);
            let (Some(seq), Some(kind)) = (req["seq"].as_u64(), req["kind"].as_str()) else {
                return err("acréscimo sem posição ou tipo".into());
            };
            match arx::lifecycle::MemRegistry::append_to(&meta, &mut chain, seq, kind, req["body"].clone()) {
                Ok(e) => {
                    let tx = match c.transaction() {
                        Ok(t) => t,
                        Err(e) => return err(e.to_string()),
                    };
                    let r = tx.execute(
                        "INSERT INTO lc_entries (scope, seq, entry) VALUES (?1, ?2, ?3)",
                        params![scope, e.seq as i64, e.to_json().to_string()],
                    );
                    match r.and_then(|_| tx.commit()) {
                        Ok(()) => Reply::Line(json!({ "ok": true, "entry": e.to_json() })),
                        Err(_) => Reply::Line(json!({ "ok": false, "conflict": true })),
                    }
                }
                Err(arx::lifecycle::AppendError::Conflict) => Reply::Line(json!({ "ok": false, "conflict": true })),
                Err(arx::lifecycle::AppendError::Refused(m)) | Err(arx::lifecycle::AppendError::Unreachable(m)) => {
                    let _ = c.execute(
                        "INSERT INTO log (line) VALUES (?1)",
                        params![format!("lifecycle {} {kind} recusado: {m}", &scope[..12.min(scope.len())])],
                    );
                    Reply::Line(json!({ "ok": false, "refused": true, "error": m }))
                }
            }
        }
        _ => err("operação de registro desconhecida".into()),
    }
}

/// O recibo assinado de um desfecho: a evidência que a âncora exige.
fn with_receipt(svc: &Svc, req: &Json, reply: Json) -> Json {
    let (Some(key), Some(wire)) = (svc.key, req["key"].as_str()) else { return reply };
    let tool = req["tool"].as_str().unwrap_or_default();
    let show = |v: &Json| v.as_str().map(String::from).unwrap_or_else(|| v.to_string());
    let (args, status, value) = match req["op"].as_str() {
        Some("call") => {
            let args = arx::lifecycle::args_hash(req["args"].as_array().map(Vec::as_slice).unwrap_or_default());
            if reply["ok"] == true {
                (args, "done", show(&reply["value"]))
            } else {
                (args, "refused", show(&reply["error"]))
            }
        }
        Some("status") => match reply["status"].as_str() {
            Some("done") => ("-".to_string(), "done", show(&reply["value"])),
            Some("not_applied") => ("-".to_string(), "not_applied", String::new()),
            _ => return reply,
        },
        _ => return reply,
    };
    let r = arx::lifecycle::Receipt::sign(&key, wire, tool, &args, status, &value);
    let mut reply = reply;
    reply["receipt"] = r.to_json();
    reply
}

fn dispatch(svc: &Svc, req: &Json) -> Reply {
    match req["op"].as_str() {
        Some("time") => time_reply(svc, req),
        Some("lifecycle") => lifecycle_reply(svc, req),
        _ => match handle(&svc.db, req) {
            Reply::Line(j) => Reply::Line(with_receipt(svc, req, j)),
            Reply::Late(j, ms) => Reply::Late(with_receipt(svc, req, j), ms),
            Reply::Silence => Reply::Silence,
        },
    }
}
